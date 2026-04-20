;; ══════════════════════════════════════════════════════
;; MissionD — Worker Pillar Execution Log
;; Parent:   .missiond/v2/drafts/gptpro/intent-worker.lisp (v0.1 starter, 待 phase-A iterate)
;; Created:  2026-04-21 (phase-0 预热 snapshot)
;; Protocol: v0.5.1 (id-counters + claims lease + audit/repair)
;;
;; 读写规则 (和 intent-memory.lisp :: helper agent-execution-coordination 一致):
;;   phase-tracker:  当前 phase 全局状态
;;   claims:         谁锁定了哪个 scope (防并发写冲突)
;;   deviations:     施工启动后真实编码偏差 (本 log 自增 D<NNN>, 不复用 memory log 的 D 号)
;;   pre-deviations: phase-0 预热 snapshot 发现的 gptpro v0.1 draft 与实况 candidate
;;   decisions:      执行期决策 DC<NNN>
;;   completions:    phase 完成凭证 COMP<NNN>
;;   issues:         阻塞/未决 I<NNN>
;;
;; ID 分配: v0.5.1 协议要求由 manager 原子分配, 在 manager-tool 上线前暂人工 claim
;; pre-deviation vs deviation: pre-deviation 只是 phase-0 snapshot 的候选 candidate,
;;                             phase-A iteration 时拍板 (升级为 design patch 或 drop)
;; ══════════════════════════════════════════════════════

(execution worker-pillar-refactor
  (meta
    :execution_id "worker-pillar-2026-04-21"
    :parent_design "intent-worker.lisp"
    :parent_status "v0.1-draft-pending-phase-A"
    :companion_of "draft"
    :opened_at "2026-04-21"
    :scope ".missiond/v2/intent-worker.lisp (当前 draft) + 对应 code:
            crates/missiond-daemon/src/workers/
            crates/missiond-daemon/src/engine/
            crates/missiond-daemon/src/flow/ (= engine/flow/)
            crates/missiond-daemon/src/llm/
            crates/missiond-daemon/src/context/
            crates/missiond-daemon/src/slot_orchestrator/
            crates/missiond-pty/src/"
    :status "phase-0-snapshot-opened")

  (id-counters
    :next-claim-id 1
    :next-deviation-id 1          ; 本执行从 D001 起, 不复用 memory log 的 D 序号
    :next-decision-id 1
    :next-issue-id 1
    :next-completion-id 1
    :next-pre-deviation-id 11)    ; P-D001..P-D010 已填; phase-A 追加从 P-D011 起

  (phase-tracker
    :current_phase "phase-0-warmup"
    :phases [phase-0-warmup phase-A-design phase-B-scan phase-C-施工 phase-D-validation phase-E-polish]
    :current_owner "主 Claude (warmup)"
    :phase-0-warmup :started-at "2026-04-21" :status "in-progress"
    :phase-A-design :status "pending"
      :intent "用 pre-deviations 驱动 gptpro v0.1 draft 迭代到 v0.2, 敲定 worker / engine / infra 在新 lisp 下的分类与章节"
    :phase-B-scan :status "pending"
      :intent "基于 v0.2 design 做代码 scan, 产 file-to-section-mapping + missing-in-code + orphan 三表"
    :phase-C-施工 :status "pending" :intent "代码向 lisp 对齐 (stage 化)"
    :phase-D-validation :status "pending" :intent "lisp↔code 双向同构校验"
    :phase-E-polish :status "pending" :intent "memory pillar cross-ref 回填 + 归档 drift-audit")

  ;; ─────────────────────────────────────────────────────
  ;; claims — phase-0 snapshot 只读, 无 claim
  ;; ─────────────────────────────────────────────────────
  (claims)

  ;; ─────────────────────────────────────────────────────
  ;; deviations — frozen lisp vs 代码实际的偏差
  ;; phase-C 施工启动后, agent 遇到具体偏差记这里
  ;; ─────────────────────────────────────────────────────
  (deviations
    ;; 尚无. phase-A 完成 design 迭代前, 正式 deviation 不累计.
    )

  ;; ─────────────────────────────────────────────────────
  ;; pre-deviations — phase-0 预热 gptpro v0.1 vs drift-audit/ground-truth candidate
  ;; phase-A iteration 时对每条拍板: 升格 D<NNN> / drop / 合并
  ;; ─────────────────────────────────────────────────────
  (pre-deviations
    (desc "phase-0 预热 snapshot 发现 gptpro v0.1 draft 与实况 (drift-audit + 磁盘真况) 的 candidate 差异")

    (P-D001
      :scope "worker 子目录分类哲学 — active-roster vs WorkerKind"
      :gptpro-v0.1-said "section 2.3 worker-cluster :: active-roster 用 (sonnet 5) (codex 1) (gemini 1) (local 12) 这种 '(category count 简介)' 的平铺 roster"
      :drift-audit-shows "workers/mod.rs 明确声明 'Directory structure is the contract: sonnet/, codex/, gemini/, local/' 且有 enum WorkerKind { Sonnet, Codex, Gemini, Local } 作为 trait BackgroundWorker::KIND 的正式 ontology. 每个 kind 对应一套 ControlTree provider dependency"
      :nature "assumption mismatch — gptpro 把 WorkerKind 降级成了 '数量 + 简介' 的 roster, 丢掉了最重要的 trait 契约与 provider dep 注入语义"
      :suggested-resolution "phase-A 把 section 2.3 改成按 WorkerKind 四分: worker-sonnet / worker-codex / worker-gemini / worker-local, 每子节独立 ingress/logic-core/egress; active-roster 仅作为该 section 的辅助字段")

    (P-D002
      :scope "local/ worker 数量"
      :gptpro-v0.1-said "active-roster 写 'local 12' — 然后列了 12 个 worker: conversation-logger / conversation-organizer / pty-event / tagger-chunker / experience-harvester / reconcile / gemini-reconcile / ast-sync / code-prefetch / codex-ingestion / gemini-logger / xjpcode-briefing"
      :drift-audit-shows "磁盘 workers/local/ 下 12 个 .rs 文件 (不含 mod.rs), 与 gptpro 计数一致. 但 drift-audit 文字段又写 'local/ (10 files, 10 spawned)', 是 drift-audit 自己写错标题. main.rs spawn 实际 12 个 local worker (ast-sync / conversation-logger / pty-event / reconcile / xjpcode-briefing / gemini-reconcile / codex-ingestion / conversation-organizer / tagger-chunker / gemini-logger + 2 非 spawn 即 code-prefetch / experience-harvester)"
      :nature "count mismatch in drift-audit 文字 (不是 gptpro 错, 是 drift-audit 自己写错 10 应 12)"
      :suggested-resolution "phase-A 直接按磁盘 12 + main.rs spawn 实况重写 local worker 列表; drift-audit 段标题 '(10 files, 10 spawned)' 顺便修正成 '(12 files, 10 spawned, 2 non-spawn)'")

    (P-D003
      :scope "sonnet/ worker 列表 — briefing_worker 已死仍被 active-roster 暗示"
      :gptpro-v0.1-said "active-roster (sonnet 5 'embedding / translation / arch-maintenance / retro / lisp-survey') 恰好 5 个, 已排除 briefing_worker"
      :drift-audit-shows "sonnet/ 磁盘 5 文件 + mod.rs, briefing_worker 已在 v1.3.0 删除. gptpro 数字正确"
      :nature "count ok — 但 gptpro 没给每个 worker 分配一条 (path ...) 逻辑, 只用一个 (path timer-worker-cycle) 粗糙概括全部 sonnet worker"
      :suggested-resolution "phase-A 为 embedding / translation / arch-maintenance / retro / lisp-survey 分别写 ingress/logic-core/egress (现只 lisp-survey 有, retro/embedding/arch-maintenance/translation 缺)")

    (P-D004
      :scope "engine 构成 — 只写 autopilot + flow-engine-v2, 漏了 memory_scheduler / workflow_executor / learning_engine 全家"
      :gptpro-v0.1-said "section 2.4 orchestration 只有 worker-lifecycle-governance + autopilot-tick + flow-engine-v2-node-execution 三条 path"
      :drift-audit-shows "engine/ 实际 3 子目录: intent_engine/ (autopilot / flow_engine / memory_scheduler / workflow_executor / gen_engine 5 file), learning_engine/ (decision_engine / decision_harvest / extraction / historical_scanner / idle_explorer / intent_analyst / timeline_analyst / gen_engine 8 file), flow/ (handlers / runner / loader 3 file + examples/)"
      :nature "missing — gptpro 完全没提 learning_engine 8 个, 没提 memory_scheduler 与 workflow_executor 这两个 intent_engine 成员, 也把 flow 与 intent_engine 混到一起"
      :suggested-resolution "phase-A 独立一个 section engine-cluster, 分 intent-engine / learning-engine / flow-engine 三个子节, 每个成员一条 path")

    (P-D005
      :scope "LLM gateway 文件清点 — gptpro 只提 sonnet / gemini / codex / minimax, 漏 driver 与 cli"
      :gptpro-v0.1-said "section 2.2 llm-gateways :: step s3 '按 model/provider 路由到 sonnet / gemini / codex / minimax(legacy)'"
      :drift-audit-shows "llm/ 磁盘 14 文件: codex_cli / gemini_cli / gemini_client / gemini_driver / gemini_file_api / gemini_pty / gen_engine / llm_gate / llm_gateway / minimax_client / minimax_gateway / mod / prompts / sonnet_gateway"
      :nature "granularity mismatch — gptpro 不知道 gemini 走 client/driver/pty/file-api 4 件套, codex 走 cli, 导致 (entry-components [LlmGateway llm_gate]) 严重不足以描述 LLM 层"
      :suggested-resolution "phase-A 在 llm-gateways section 补 gemini-4-piece 与 codex-cli-wrapper, 并显式声明 minimax 'legacy, 仅 OpenClaw 外部保留' 的架构注释")

    (P-D006
      :scope "infra layer 缺席 — gptpro 完全没认知 crates/missiond-daemon/src/infra/ 目录"
      :gptpro-v0.1-said "(无任何 infra section)"
      :drift-audit-shows "infra/ 7 文件: aiops / daemon_stats / ingestion_router / ipc_handler / mcp_client / message_handler / session_util. 其中 ingestion_router 被 drift-audit 标为 pillar 三/二 交界. 其余 6 个 drift-audit 标 'pillar 六 系统层 SSOT'"
      :nature "missing section — worker pillar 逻辑上不包含 infra (infra 归 system pillar), 但 worker 大量穿越 infra (e.g. conversation_logger → ingestion_router → message_handler). gptpro 对 cross-pillar boundary 没任何声明"
      :suggested-resolution "phase-A 在 pillar-egress 里补一条 cross-pillar-notes: 明确 infra/ 属 system pillar 但是 worker 的 data-plane 穿行路径, 并在相关 path 的 logic-core 注明走 ingestion_router / message_handler")

    (P-D007
      :scope "PTY 目录 — gptpro targets 写 crates/missiond-pty/src/* 但未列具体模块"
      :gptpro-v0.1-said ":targets [crates/missiond-pty/src/* crates/missiond-daemon/src/slot_manager/ crates/missiond-daemon/src/slot_orchestrator/]"
      :drift-audit-shows "missiond-pty/src/: anomaly / extractor / lib / manager / screenshot / session 6 文件. slot_orchestrator/ 11 文件: agent / cc_controller / claude_code / controller / gemini_cli / gemini_controller / gen_engine / perm_injector / spawner / types. slot_manager/ 则是 missiond-daemon 内部模块, drift-audit 中无独立清点"
      :nature "granularity gap — gptpro 用 glob 概括, phase-A 需具体到每个 module 归于哪条 path"
      :suggested-resolution "phase-A 在 section 2.1 pty 补 :file-layout 子块, 明确 session-lifecycle / jsonl-ingestion / slot-dispatch 三 path 分别落在哪些具体 .rs")

    (P-D008
      :scope "跨 pillar 交叉 — memory pillar v0.5.1 与 worker-pillar 的 table cross-ref 未对齐"
      :gptpro-v0.1-said "pillar-egress :: egress-1 '写回 memory pillar: conversations / board / KB / slots / observability' (单行总结, 无表级精度)"
      :drift-audit-shows "board_tasks 表 24+ caller, event_log 40+ caller, conversation_messages 多路径, skill 表由 lisp_survey_worker + code_prefetch 读. memory pillar 二 2.3 workers/... 应该对应声明这些 cross-ref"
      :nature "contract gap — worker → memory 的数据流缺显式 table-level contract, pillar 间 SSOT 无法闭环"
      :suggested-resolution "phase-A 参照 memory intent-memory.lisp 的 writer/reader 注册表格式, 在 worker-pillar 的每个 path 的 egress 里显式列 'writes: table-names[]' 和 'reads: table-names[]'")

    (P-D009
      :scope "context pipeline 分类归属"
      :gptpro-v0.1-said "section 2.2 llm-gateways 里含 (path context-assembly), 把 context_pipeline / context_budget / slot_env 归 LLM gateway 层"
      :drift-audit-shows "context/ 磁盘 7 文件: claude_md_sync / context_budget / context_pipeline / pure_budget/ / slot_env / topology_map / mod. 含 pure_budget/ 子目录 + 独立 topology_map.rs + claude_md_sync.rs — 比 gptpro 认知更广"
      :nature "file footprint gap + classification question — context layer 是应该归 llm-gateways 还是自成一节?"
      :suggested-resolution "phase-A 讨论: (A) context 独立一节 section context-assembly; 或 (B) 继续并到 llm-gateways 但补全 7 文件引用. 建议 A, 因为 claude_md_sync 与 topology_map 明显不是 LLM 层")

    (P-D010
      :scope "zombie 文件与 active-roster 一致性"
      :gptpro-v0.1-said "(known-footprint-drift :note '旧代码足迹里仍可能看到 briefing_worker / step_narrator 等历史文件; 设计上按 active roster 管, zombie 文件列入 pillar 二清理清单')"
      :drift-audit-shows "zombie files: code_prefetch.rs (active 但非 spawn, 由 context_pipeline 调用) + experience_harvester.rs (疑似计划功能未 spawn). briefing_worker / step_narrator 已从磁盘删除"
      :nature "roster 定义偏差 — gptpro 把 experience-harvester 列入 local 12 active, 但 drift-audit 显示它未 spawn 应是 zombie; code-prefetch 归类也有歧义 (gptpro 算 active, drift-audit 说 '活跃但隐含耦合 非标准 BackgroundWorker')"
      :suggested-resolution "phase-A 决策: (a) experience-harvester 升级为 '计划功能' 独立标签 / 或移出 roster; (b) code-prefetch 独立一条 path on-demand-retrieval 而非归 event-driven-worker-cycle")
    )

  (decisions)

  (completions)

  (issues)

  (derived-indexes
    (active_claims [])
    (unresolved_deviations [])
    (open_issues [])
    (completed_phases [])
    (pre_deviations_count 10))

  ;; ─────────────────────────────────────────────────────
  ;; phase-0-snapshot — 预热基线
  ;; ─────────────────────────────────────────────────────
  (phase-0-snapshot
    :taken-at "2026-04-21"
    :sources
      ["gptpro v0.1 intent-worker.lisp (273L)"
       "drift-audit-2026-04-21.md (worker 19 文件 / 17 spawn / engine intent 5 + learning 8 / flow 3 / infra 7 文件)"
       "memory pillar v0.5.1 已有的 worker cross-ref (intent-memory.lisp :: 二 2.3)"
       "磁盘真况: workers/{local(12), sonnet(5), gemini(1), codex(1)} + engine/{intent_engine, learning_engine, flow} + llm(14) + context(7) + slot_orchestrator(11) + pty(6) + infra(7)"]
    :ground-truth-counts
      (workers-on-disk 19)
      (workers-spawned-in-main 17)
      (zombie-files 2 "code_prefetch(by context_pipeline) + experience_harvester(未 spawn)")
      (engine-intent 5 "autopilot / flow_engine / memory_scheduler / workflow_executor / gen_engine")
      (engine-learning 8 "decision_engine / decision_harvest / extraction / historical_scanner / idle_explorer / intent_analyst / timeline_analyst / gen_engine")
      (engine-flow 3 "handlers / runner / loader + examples/")
      (infra 7 "aiops / daemon_stats / ingestion_router / ipc_handler / mcp_client / message_handler / session_util")
      (llm 14)
      (context 7)
      (slot-orchestrator 11)
      (pty 6)
    :pre-deviations-count 10
    :gptpro-coverage-gaps
      (missing-sections ["infra (0 认知)" "learning-engine 8 文件 (0 认知)" "memory_scheduler / workflow_executor (漏)"])
      (undergranular ["sonnet/ 5 worker 合并成一条 timer-worker-cycle" "llm/ 14 文件被压成 4 provider"])
      (classification-questions ["context → llm 还是独立节?" "experience-harvester 算 active 还是 zombie?" "code-prefetch 归 cycle 还是 on-demand?"])
    :phase-A-iteration-starting-point
      "建议 phase-A 按 P-D001→P-D010 顺序 iterate:
         1. 最先改 P-D001 (WorkerKind 四分 section), 是 worker pillar 骨架;
         2. 然后 P-D004 (engine-cluster 独立 + learning-engine 补齐), 是 orchestration 骨架;
         3. 然后 P-D006 + P-D009 (infra cross-pillar note + context 独立节), 补缺;
         4. 然后 P-D003 + P-D007 (每 worker 每 module 细化 path);
         5. 最后 P-D008 (table-level cross-ref 回填 memory pillar) 作 phase-E polish;
         6. P-D002 + P-D010 作 drift-audit 文字 + zombie 政策小修正."))
))
