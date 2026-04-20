;; ══════════════════════════════════════════════════════
;; MissionD — Worker Pillar Execution Log
;; Parent:   .missiond/v2/intent-worker.lisp (v0.3 phase-B informed, 1831 行)
;; Created:  2026-04-21 (phase-0 预热 snapshot)
;; Updated:  2026-04-21 (phase-B informed — 吸收 8 份老图, v0.2→v0.3)
;; Protocol: v0.5.1 (id-counters + claims lease + audit/repair)
;;
;; 读写规则 (和 intent-memory.lisp :: helper agent-execution-coordination 一致):
;;   phase-tracker:  当前 phase 全局状态
;;   claims:         谁锁定了哪个 scope (防并发写冲突)
;;   deviations:     施工启动后真实编码偏差 (本 log 自增 D<NNN>, 不复用 memory log 的 D 号)
;;   pre-deviations: phase-0 预热 snapshot 的 candidate (已全部升格到 D001-D010)
;;   decisions:      执行期决策 DC<NNN>
;;   completions:    phase 完成凭证 COMP<NNN>
;;   issues:         阻塞/未决 I<NNN>
;;
;; ID 分配: v0.5.1 协议要求由 manager 原子分配, 在 manager-tool 上线前暂人工 claim
;; ══════════════════════════════════════════════════════

(execution worker-pillar-refactor
  (meta
    :execution_id "worker-pillar-2026-04-21"
    :parent_design "intent-worker.lisp"
    :parent_status "v0.3-phase-B-informed"
    :companion_of "design"
    :opened_at "2026-04-21"
    :phase-A-completed-at "2026-04-21"
    :phase-B-informed-at "2026-04-21"
    :scope ".missiond/v2/intent-worker.lisp + 对应 code:
            crates/missiond-daemon/src/workers/
            crates/missiond-daemon/src/engine/
            crates/missiond-daemon/src/engine/flow/
            crates/missiond-daemon/src/llm/
            crates/missiond-daemon/src/context/
            crates/missiond-daemon/src/slot_orchestrator/
            crates/missiond-pty/src/
            crates/missiond-core/src/semantic_parsing/
            crates/semantic-terminal-napi/src/"
    :status "phase-B-informed, phase-C-施工-ready")

  (id-counters
    :next-claim-id 1
    :next-deviation-id 13         ; D001..D012 已用, 下一个从 D013 起
    :next-decision-id 16          ; DC001..DC015 已用 (DC011-DC015 对应 Q-B1..Q-B5), 下一个从 DC016 起
    :next-issue-id 10             ; I001..I009 已用, 下一个从 I010 起
    :next-completion-id 3         ; COMP001 (phase-A) + COMP002 (phase-B-informed) 已用
    :next-pre-deviation-id 11)    ; P-D001..P-D010 已全部升格, 无新增

  (phase-tracker
    :current_phase "phase-C-施工 (pending 指挥官 kick off)"
    :phases [phase-0-warmup phase-A-design phase-B-informed phase-C-施工 phase-D-validation phase-E-polish]
    :current_owner "主 Claude (v0.3 just written, waiting for kick)"
    :phase-0-warmup :started-at "2026-04-21" :completed-at "2026-04-21" :status "completed"
      :output "10 pre-deviations (升格 D001-D010) + drift-audit 配套数据"
    :phase-A-design :started-at "2026-04-21" :completed-at "2026-04-21" :status "completed"
      :gptpro-delivery "intent-worker-v0.2.lisp (1430 行), Q1-Q10 全答, 10 pre-D disposition 明确"
      :主-Claude-followup "3 处 fs inference 修正, 1434 行正位到 .missiond/v2/intent-worker.lisp"
    :phase-B-informed :started-at "2026-04-21" :completed-at "2026-04-21" :status "completed"
      :scope "本会话主驾, 不再依赖 gptpro (gptpro 无法查代码)"
      :input "8 份 .missiond/intent-pillar-*.lisp 老图 + 指挥官 5 问题"
      :output "intent-worker.lisp v0.3 (1831 行, 13 大变更, paren 696=696)"
      :key-changes "section pty 重构 5 subsection / 新 section xjp-router-gateway / engine-cluster 瘦身 / functional-groups / mcp-surface-to-tools / event-categories 9 类 / flow-engine v1-v2 区分 / ControlTree 6 层 / bootstrap depends-graph / sole-spawn-bottleneck / learned-permissions / registered-tasks / 双重归属"
    :phase-C-施工 :status "ready-to-start" :intent "代码向 v0.3 lisp 对齐 — xjp_router_client 新建 / learned_permissions 可能补接口 / flow-engine v1 迁 intent-layer / learning-engine primary-ownership 搬家"
    :phase-D-validation :status "pending" :intent "lisp↔code 双向同构校验"
    :phase-E-polish :status "pending" :intent "memory pillar cross-ref 回填 + table-level contract 入 intent-memory.lisp + 归档 drift-audit")

  ;; ─────────────────────────────────────────────────────
  ;; claims — phase-B scan 尚未启动, 暂空
  ;; ─────────────────────────────────────────────────────
  (claims)

  ;; ─────────────────────────────────────────────────────
  ;; deviations — phase-A 完成, D001-D010 为 pre-deviation 升格, D011 为主 Claude 补丁
  ;; ─────────────────────────────────────────────────────
  (deviations
    (D001
      :升格自 P-D001
      :scope "worker 子目录分类哲学 — active-roster vs WorkerKind"
      :disposition "accept"
      :applied-in "v0.2 section worker-cluster 改成 4 subsection (worker-sonnet / worker-codex / worker-gemini / worker-local)"
      :effect "把目录契约 + BackgroundWorker::KIND + ControlTree provider 注入重新抬升为骨架级语义")

    (D002
      :升格自 P-D002
      :scope "local/ worker 数量 (文字 bug)"
      :disposition "accept"
      :applied-in "v0.2 worker-local 显式写 :disk-count 12 :spawned-count 10 :on-demand-count 1 :planned-count 1"
      :followup "drift-audit 文字 typo 已由 commit eddb606 修正")

    (D003
      :升格自 P-D003
      :scope "sonnet/ worker path 粒度"
      :disposition "accept"
      :applied-in "v0.2 worker-sonnet 子节 5 条独立 path (embedding / translation / arch-maintenance / retro / lisp-survey)"
      :effect "每 worker 的 ingress/logic-core/egress 分立, 不再用粗糙 timer-worker-cycle 压一份")

    (D004
      :升格自 P-D004
      :scope "engine 构成 — learning-engine 全家漏掉"
      :disposition "accept"
      :applied-in "v0.2 独立顶级 section engine-cluster, 分 3 subsection (intent-engine 4 path / learning-engine 3 sub × 共 7 path / flow-engine 3 path), gen_engine.rs 作 Forge shell 单列不给 path"
      :effect "engine 家族从 3 path → 14 path, 骨架最大漏洞补齐")

    (D005
      :升格自 P-D005
      :scope "LLM gateway 文件清点 — 粒度严重不足"
      :disposition "accept"
      :applied-in "v0.2 llm-gateways section 细分 6 条 path (routing / sonnet / gemini-unified / codex-cli / minimax-legacy / prompt-template)"
      :effect "llm/ 14 文件每个归到 entry-components, gemini 4 件套 + codex cli + minimax 双层 + sonnet + prompts + llm_gate + gen_engine 全部在册")

    (D006
      :升格自 P-D006
      :scope "Infra 层缺席"
      :disposition "accept"
      :applied-in "v0.2 pillar-egress 新 cross-pillar-notes::system-infra 独立块, 并在 conversation-jsonl-ingestion / pty-event-worker-cycle / claude-slot-dispatch 等 path 的 logic-core 里双写声明穿越 ingestion_router / message_handler / session_util"
      :effect "worker → system infra 的 data-plane 穿越点显式化, 不再暗耦合")

    (D007
      :升格自 P-D007
      :scope "PTY + slot_orchestrator 粒度"
      :disposition "accept"
      :applied-in "v0.2 section pty 的 4 个子段 (transport-files / semantic-parser-files / orchestrator-files / bridge-files) 列出具体文件, slot_orchestrator 11 文件 + pty 6 文件全部在 entry-components, 无 glob"
      :followup "slot_manager/ 推断为独立目录的错误由 D011 修正")

    (D008
      :升格自 P-D008
      :scope "跨 pillar table-level cross-ref 契约"
      :disposition "accept"
      :applied-in "v0.2 每 path 的 egress 含 :writes / :reads / :via-bus / :memory-cross-ref; 每 WorkerKind 子节末含 contract-summary 便利审阅"
      :effect "worker → memory 的 table-level 契约可与 intent-memory.lisp v0.5.1 frozen 对齐")

    (D009
      :升格自 P-D009
      :scope "context pipeline 分类归属"
      :disposition "accept"
      :applied-in "v0.2 独立 section context-assembly (slot-env-build / claude-md-managed-sync / topology-map-resolution / context-bundle-assembly 4 path), 与 llm-gateways / worker-cluster / engine-cluster 并列"
      :effect "claude_md_sync 与 topology_map 脱离 LLM gateway 分类, 边界更接近真实代码")

    (D010
      :升格自 P-D010
      :scope "zombie 文件与 active-roster 一致性"
      :disposition "partial"
      :applied-in "v0.2 引入 :lifecycle-style 四分 (spawned / on-demand / planned / zombie-deleted) + zombie-ledger 独立块 + :active-definition 'spawned ∪ on-demand-call'"
      :remaining "experience_harvester phase-A 保留 path 设计骨架; phase-B 需确认它是 planned 功能还是将删 prototype (见 I005)")

    (D011
      :origin "主 Claude 2026-04-21 phase-A 集成补丁"
      :scope "gptpro v0.2 的 3 处 fs inference 错误"
      :disposition "fixed-in-integration"
      :applied-in ".missiond/v2/intent-worker.lisp (vs .missiond/v2/drafts/gptpro/intent-worker-v0.2.lisp 原版)"
      :details
        [(semantic-terminal-dir
           :gptpro-claimed "semantic-terminal/src/{patterns,state,gemini_state,fingerprint,confirm,tool,status,title}.rs"
           :ground-truth "目录不存在, 旧 8 文件已经 Forge 冲压合入 missiond-core/src/semantic_parsing/{generated,custom,mod}.rs + semantic-terminal-napi/src/lib.rs"
           :fix "semantic-parser-files 4 文件替换, 加 :semantic-parser-legacy-footprint 注释段")
         (context-retrieval-rs
           :gptpro-claimed "crates/missiond-daemon/src/context/retrieval.rs"
           :ground-truth "文件不存在, 检索融合由 context_pipeline + code_prefetch + handlers/knowledge/kb 协作"
           :fix "retrieval-fusion path 的 entry-components 删除 retrieval.rs; need-more-ground-truth 同步更新")
         (slot-manager-dir
           :gptpro-claimed "crates/missiond-daemon/src/slot_manager/"
           :ground-truth "目录不存在, 已合入 slot_orchestrator/"
           :fix "section pty 的 :need-more-ground-truth 改成 '目录实际不存在, phase-B 确认残留引用清理'; file-level item 同步")])

    (D012
      :origin "主 Claude 2026-04-21 phase-B-informed 大 refactor"
      :scope "v0.2 → v0.3: 吸收 8 份老图 ground-truth + 修指挥官 5 问题"
      :disposition "applied-in-v0.3"
      :trigger "指挥官 5 问题 + 发现 v0.2 压缩了 v1 pillar 级的详尽设计"
      :gptpro-skipped "gptpro 无法查代码, 本会话自己驾驶 v0.3 而不是发 brief"
      :summary
        [(v0.3-lines "1831 行 (v0.2 1434 → +397)")
         (paren-balance "696 = 696 ✓")
         (change-count 13)]
      :changes
        [(change-1 "section pty 重构为 5 subsection — pty-transport / semantic-parser / pty-state-machine / slot-orchestrator / learned-permissions")
         (change-2 "新 (section xjp-router-gateway) — embedding 走 xjp-router, sonnet-priority-gateway 去 embedding")
         (change-3 "engine-cluster 瘦身: learning-engine 标 primary-ownership intent-layer pillar; flow-engine v1 迁 intent-layer; v2 留 worker")
         (change-4 "worker-local 加 :functional-groups (6 组: cli-ingestion / 认知管道 / observability-log / code-intel / pty-runtime-hook / meta-briefing)")
         (change-5 "pillar-egress 新 :mcp-surface-to-tools (14 compute-tools + 4 sysinfra-tools 完整映射)")
         (change-6 "event-categories 补到 9 类 (v0.2 只 5 类)")
         (change-7 "flow-engine v2 补 7 fail-fast-invariants + 5 node-types 详细 spec")
         (change-8 "ControlTree 6 字段 + cascade-priority 3 层 + ControlManager watch channel 零轮询 + persistence")
         (change-9 "bootstrap 6 phase + depends-graph 9 关系 + app-state fields + supervisor")
         (change-10 "slot-orchestrator 加 sole-spawn-bottleneck 不变量 (10 callers 全列) + slot-config-fields + registered-tasks 4")
         (change-11 "learned-permissions 完整: multi-scope 4 (global/role/project/slot) + REQUIRES_PARAM_PATTERN + permission-persistence flow 6 步 + read/write path 分离 + mcp-merged-view")
         (change-12 "semantic-parser: parser-pipeline 5-stage + 8 parser components (claude-code / gemini / fingerprint / confirm / tool / status / title / helpers) + semantic-terminal-napi 前端壳")
         (change-13 "pty-state-machine: pty-session FSM 8 states + 14 transitions 完整转载; 其他 5 FSM 标归属 (memory/intent-layer pillar)")])
  )

  ;; ─────────────────────────────────────────────────────
  ;; pre-deviations — 全部升格, 原文保留作 audit trail
  ;; ─────────────────────────────────────────────────────
  (pre-deviations
    (desc "phase-0 预热 snapshot 10 候选, phase-A 完成时已全部升格 D001-D010")

    (P-D001 :升格 D001 :scope "WorkerKind vs active-roster")
    (P-D002 :升格 D002 :scope "local/ 数量 (文字 bug)")
    (P-D003 :升格 D003 :scope "sonnet/ path 粒度")
    (P-D004 :升格 D004 :scope "engine learning-engine 漏掉")
    (P-D005 :升格 D005 :scope "llm-gateway 粒度")
    (P-D006 :升格 D006 :scope "infra 缺席")
    (P-D007 :升格 D007 :scope "pty + slot_orch 粒度")
    (P-D008 :升格 D008 :scope "table-level cross-ref 契约")
    (P-D009 :升格 D009 :scope "context 独立节")
    (P-D010 :升格 D010 :scope "zombie 与 active-roster 一致性, partial 处置")

    (note "完整原文见 git history at commit 91f02da (phase-0-snapshot), 本 log 从 phase-A 起以 D001-D010 为准"))

  ;; ─────────────────────────────────────────────────────
  ;; decisions — phase-A 共 10 条 DC001-DC010 对应 brief §6 Q1-Q10
  ;; ─────────────────────────────────────────────────────
  (decisions
    (DC001
      :related-Q Q1
      :related-D D001
      :decision "worker-cluster 按 WorkerKind 四分 (Sonnet/Codex/Gemini/Local)"
      :decided-by "gptpro phase-A-decisions + 指挥官 approve"
      :decided-at "2026-04-21")

    (DC002
      :related-Q Q2
      :related-D D004
      :decision "engine-cluster 独立为顶级 section, 与 worker-cluster 并列"
      :decided-by "gptpro phase-A-decisions"
      :decided-at "2026-04-21")

    (DC003
      :related-Q Q3
      :related-D D004
      :decision "learning_engine 按 decision/extraction/analysis 3 sub group"
      :decided-by "gptpro phase-A-decisions"
      :decided-at "2026-04-21")

    (DC004
      :related-Q Q4
      :related-D D005
      :decision "Gemini 采用 1 条 path `gemini-unified-gateway` + entry-components 列 5 个文件 (driver/cli/client/pty/file_api)"
      :decided-by "gptpro phase-A-decisions (accept-with-clarification)"
      :decided-at "2026-04-21")

    (DC005
      :related-Q Q5
      :related-D D006
      :decision "infra cross-pillar-notes 独立块 + 相关 path step 双写"
      :decided-by "gptpro phase-A-decisions"
      :decided-at "2026-04-21")

    (DC006
      :related-Q Q6
      :related-D D008
      :decision "R/W table 以 path-level egress 为主, 每 WorkerKind 子节末补 contract-summary (审阅便利)"
      :decided-by "gptpro phase-A-decisions"
      :decided-at "2026-04-21")

    (DC007
      :related-Q Q7
      :related-D D009
      :decision "context 独立为 (section context-assembly), 与 llm-gateways 并列"
      :decided-by "gptpro phase-A-decisions"
      :decided-at "2026-04-21")

    (DC008
      :related-Q Q8
      :related-D D010
      :decision "active = spawned ∪ on-demand-call; :lifecycle-style 四分 (spawned/on-demand/planned/zombie-deleted)"
      :decided-by "gptpro phase-A-decisions"
      :decided-at "2026-04-21")

    (DC009
      :related-Q Q9
      :decision "v0.2 行数区间 1000-1400, 最终 1434 行 (含主 Claude 集成补丁 ~50 行)"
      :decided-by "gptpro phase-A-decisions (accept-with-adjustment)"
      :decided-at "2026-04-21")

    (DC010
      :related-Q Q10
      :decision "保留 :actual-state-sources 顶部元信息, 主材料指向 .missiond/v2/{drift-audit, intent-pillar-source-index, worker-pillar-execution}, 旧图退到 :historical-footprint-sources"
      :decided-by "gptpro phase-A-decisions"
      :decided-at "2026-04-21")

    ;; ── phase-B-informed 决策 DC011-DC015 ──
    (DC011
      :related-Q-B Q-B1
      :related-D D012
      :decision "embedding 独立 section xjp-router-gateway. sonnet-priority-gateway 去 embedding"
      :rationale "Windows 12900KF + QWEN3 路由, 已接通但未接入 missiond"
      :code-status "pending HTTP client (I006)"
      :decided-by "指挥官 2026-04-21"
      :decided-at "2026-04-21")

    (DC012
      :related-Q-B Q-B2
      :decision "worker-local 加 :functional-groups 6 组"
      :rationale "cli-ingestion / 认知管道 / observability-log / code-intel / pty-runtime-hook / meta-briefing 横切"
      :decided-by "指挥官"
      :decided-at "2026-04-21")

    (DC013
      :related-Q-B Q-B3
      :decision "section pty 重构为 5 subsection"
      :rationale "吸收 intent-pillar-semantic-parser + state-machines + transport-bootstrap + engines 老图的详尽设计"
      :decided-by "指挥官"
      :decided-at "2026-04-21")

    (DC014
      :related-Q-B Q-B4
      :decision "engine-cluster 分拆: runtime-mechanics 留 worker, 学习/规划逻辑归 intent-layer (含 flow-engine v1 + learning-engine 7 sub); lisp-survey/arch-maintenance 双重归属"
      :rationale "v2 intent.lisp 已声明 intent-layer 拥有 lisp files + specs + workflows + lisp-survey-worker component"
      :follow-up "intent-layer pillar phase-A 时正式迁移 (I007)"
      :decided-by "指挥官"
      :decided-at "2026-04-21")

    (DC015
      :related-Q-B Q-B5
      :decision "pillar-egress 新 :mcp-surface-to-tools"
      :coverage "14 compute-tools + 4 sysinfra-tools"
      :rationale "对齐 memory pillar v0.5.1 的 :mcp-surface 模式"
      :decided-by "指挥官"
      :decided-at "2026-04-21"))

  ;; ─────────────────────────────────────────────────────
  ;; completions
  ;; ─────────────────────────────────────────────────────
  (completions
    (COMP001
      :phase "phase-A-design"
      :completed-at "2026-04-21"
      :artifacts
        [".missiond/v2/intent-worker.lisp (1434 行, phase-A frozen)"
         ".missiond/v2/drafts/gptpro/intent-worker-v0.2.lisp (gptpro 原始归档)"
         ".missiond/v2/drafts/gptpro/intent-worker.lisp (v0.1 starter 归档)"
         ".missiond/v2/drafts/gptpro/worker-pillar-phase-A-brief.md (493 行本会话施工反馈包)"]
      :meta
        (phase-A-rounds 1)
        (gptpro-bytes "~80 KB v0.2 delivery")
        (pre-deviations-resolved 10)
        (fs-inference-corrections 3)
      :sign-off-by "指挥官 + 主 Claude (2026-04-21)")

    (COMP002
      :phase "phase-B-informed"
      :completed-at "2026-04-21"
      :artifacts
        [".missiond/v2/intent-worker.lisp (1831 行 v0.3)"
         ".missiond/v2/worker-pillar-execution.lisp (D012 + DC011-DC015 + COMP002 + I006-I009 更新)"]
      :meta
        (phase-B-rounds 1)
        (gptpro-role "跳过 — 本次无需 gptpro, 本会话自己吸收 8 份老图")
        (change-count 13)
        (added-lines 397)
        (paren-balance "696 = 696 ✓")
        (sources-absorbed "8 份 .missiond/intent-pillar-*.lisp 老图 + 指挥官 5 问题 Q-B1..Q-B5")
      :sign-off-by "指挥官 + 主 Claude (2026-04-21)"))

  ;; ─────────────────────────────────────────────────────
  ;; issues — phase-B 要解决的 5 个 need-more-ground-truth
  ;; ─────────────────────────────────────────────────────
  (issues
    (I001
      :source "gptpro v0.2 need-more-ground-truth item 1"
      :scope "slot_manager/ 残留清理"
      :question "slot_manager 目录虽已并入 slot_orchestrator, 是否还有代码内的 mod/use 引用需清理?"
      :phase "phase-B-scan"
      :priority "low"
      :resolver-hint "grep -r 'slot_manager' crates/ 找残留")

    (I002
      :source "gptpro v0.2 need-more-ground-truth item 2"
      :scope "workflow_executor.rs 的表级 R/W 契约"
      :question "workflow_executor 具体读写哪些 DB 表? phase-A 材料只给 path 没给表"
      :phase "phase-B-scan"
      :priority "medium"
      :resolver-hint "Read workers 矩阵生成 agent 未扫 engine/, 派 agent 补扫")

    (I003
      :source "gptpro v0.2 need-more-ground-truth item 3"
      :scope "learning_engine 除 intent_analyst 外 7 个文件的 precise table contract"
      :question "decision_engine / decision_harvest / extraction / historical_scanner / idle_explorer / timeline_analyst 各读写哪些 DB 表?"
      :phase "phase-B-scan"
      :priority "medium"
      :resolver-hint "派 agent 扫 engine/learning_engine/ 7 文件的 sqlx/.execute/trait 调用")

    (I004
      :source "gptpro v0.2 need-more-ground-truth item 4"
      :scope "retrieval-fusion 的真实文件绑定"
      :question "retrieval-fusion path 除 context_pipeline + code_prefetch + handlers/knowledge/kb 外, 是否有专门的 fusion ranker 文件?"
      :phase "phase-B-scan"
      :priority "medium"
      :resolver-hint "grep 'fusion' / 'hybrid' 在 context/ 与 workers/ 下")

    (I005
      :source "gptpro v0.2 need-more-ground-truth item 5"
      :scope "experience_harvester.rs 去留 (D010 partial 的 followup)"
      :question "experience_harvester 是 planned 功能还是未接线 prototype? 是否要从磁盘删除?"
      :phase "phase-C-施工"
      :priority "low"
      :resolver-hint "Read experience_harvester.rs 全文 + grep 所有 import 判断 callers")

    ;; ── phase-B-informed 新发现的 issues I006-I009 ──
    (I006
      :source "v0.3 DC011 decision 附带"
      :scope "xjp_router_client 实际落点"
      :question "crates/missiond-daemon/src/llm/xjp_router_client.rs 还是别处? Cargo.toml 依赖是否够 (reqwest?)?"
      :phase "phase-C-施工"
      :priority "high"
      :resolver-hint "phase-C 启动时新建文件 + 配置 endpoint + auth_token")

    (I007
      :source "v0.3 DC014 decision 附带"
      :scope "intent-layer pillar phase-A 后的迁移动作"
      :question "board-phase-engine + learning-engine 7 sub 正式迁离 worker pillar 的执行时机和负责人?"
      :phase "phase-E-polish (intent-layer phase-A 后)"
      :priority "medium"
      :resolver-hint "intent-layer pillar phase-A 完成后, worker 侧做 cleanup commit")

    (I008
      :source "v0.3 boundary-shift 附带"
      :scope "flow-engine v1 (flow_engine.rs) 具体执行逻辑"
      :question "项目-lifecycle phases 的推进细节 — intent-layer pillar phase-A 需吸收"
      :phase "intent-layer pillar phase-A"
      :priority "medium"
      :resolver-hint "intent-layer phase-A 时主 Claude 扫 flow_engine.rs + autopilot 的调用")

    (I009
      :source "v0.3 slot-orchestrator subsection 未完全展开"
      :scope "compute_slot vs slot_orchestrator 并发 FSM"
      :question "EXCLUDED_ROLES + persistent Mutex + ephemeral 信号量的完整 FSM 是否需独立文档?"
      :phase "phase-E-polish (若指挥官要求)"
      :priority "low"
      :resolver-hint "可做, 但当前 v0.3 slot-orchestrator 已较完整"))

  (derived-indexes
    (active_claims [])
    (unresolved_deviations [D010-partial])
    (open_issues [I001 I002 I003 I004 I005 I006 I007 I008 I009])
    (completed_phases [phase-0-warmup phase-A-design phase-B-informed])
    (pre_deviations_upgraded_count 10)
    (decisions_count 15)
    (deviations_count 12)
    (completions_count 2))

  ;; ─────────────────────────────────────────────────────
  ;; phase-A-completion-report — 本轮施工 summary
  ;; ─────────────────────────────────────────────────────
  (phase-A-completion-report
    :rounds 1
    :gptpro-v0.2-size "1430 行 → 1434 行 (主 Claude fs-inference 集成补丁)"
    :骨架改造
      ["section worker-cluster 按 WorkerKind 四分 (v0.1 平铺 roster → v0.2 4 subsection)"
       "section engine-cluster 独立顶级 (v0.1 埋在 orchestration → v0.2 含 14 path)"
       "section context-assembly 独立 (v0.1 并入 llm → v0.2 4 path)"
       "cross-pillar-notes::system-infra 独立块 + path 内双写"
       "lifecycle-style 四分字段"
       "每 path egress 含 :writes/:reads/:via-bus/:memory-cross-ref"
       "每 WorkerKind 子节末 contract-summary"]
    :质量指标
      (paren-balance "513 = 513 ✓")
      (Q-coverage "Q1-Q10 全答 ✓")
      (pre-deviation-coverage "10/10 处置 (9 accept + 1 partial) ✓")
      (file-inference-errors "3 修 (semantic-terminal / context/retrieval / slot_manager) ✓")
    :remaining-for-phase-B
      ["5 issues (I001-I005) 的 need-more-ground-truth 扫描"
       "D010 partial 的 experience_harvester 去留 (I005)"
       "file-to-section-mapping 全量 binds-to 生成"
       "missing-in-code / orphan 检测"]
    :next-action
      "派 agent 扫 engine/ (intent + learning) + slot_manager 残留 + retrieval-fusion 真实 caller, 完成 phase-B-scan-report")

  (phase-0-snapshot
    :archived "保留原 snapshot 文本 (gptpro 基线数据), phase-B 可回看"
    :note "原 phase-0-snapshot 详细内容见 git history at commit 91f02da")
)
