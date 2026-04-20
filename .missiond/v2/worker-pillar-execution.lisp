;; ══════════════════════════════════════════════════════
;; MissionD — Worker Pillar Execution Log
;; Parent:   .missiond/v2/intent-worker.lisp (v0.2 integrated, phase-A frozen)
;; Created:  2026-04-21 (phase-0 预热 snapshot)
;; Updated:  2026-04-21 (phase-A completed, phase-B-scan ready)
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
    :parent_status "v0.2-phase-A-integrated"
    :companion_of "design"
    :opened_at "2026-04-21"
    :phase-A-completed-at "2026-04-21"
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
    :status "phase-A-completed, phase-B-scan-ready")

  (id-counters
    :next-claim-id 1
    :next-deviation-id 12         ; D001..D011 已用, 下一个从 D012 起
    :next-decision-id 11          ; DC001..DC010 已用, 下一个从 DC011 起
    :next-issue-id 6              ; I001..I005 已用, 下一个从 I006 起
    :next-completion-id 2         ; COMP001 已用, 下一个从 COMP002 起
    :next-pre-deviation-id 11)    ; P-D001..P-D010 已全部升格, 无新增

  (phase-tracker
    :current_phase "phase-B-scan"
    :phases [phase-0-warmup phase-A-design phase-B-scan phase-C-施工 phase-D-validation phase-E-polish]
    :current_owner "主 Claude (phase-B scan 待启)"
    :phase-0-warmup :started-at "2026-04-21" :completed-at "2026-04-21" :status "completed"
      :output "10 pre-deviations (升格 D001-D010) + drift-audit 配套数据"
    :phase-A-design :started-at "2026-04-21" :completed-at "2026-04-21" :status "completed"
      :gptpro-delivery "intent-worker-v0.2.lisp (1430 行), Q1-Q10 全答, 10 pre-D disposition 明确"
      :主-Claude-followup "3 处 fs inference 修正, 1434 行正位到 .missiond/v2/intent-worker.lisp"
    :phase-B-scan :status "ready-to-start"
      :intent "基于 v0.2 frozen design, 对 5 个 need-more-ground-truth 项做 agent scan: slot_manager 残留 / workflow_executor R/W / learning_engine 精确表契约 / retrieval-fusion 真实绑定 / experience_harvester 去留"
    :phase-C-施工 :status "pending" :intent "代码向 lisp 对齐 (stage 化)"
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
      :origin "主 Claude 2026-04-21 集成补丁"
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
      :decided-at "2026-04-21"))

  ;; ─────────────────────────────────────────────────────
  ;; completions
  ;; ─────────────────────────────────────────────────────
  (completions
    (COMP001
      :phase "phase-A-design"
      :completed-at "2026-04-21"
      :artifacts
        [".missiond/v2/intent-worker.lisp (1434 行, frozen)"
         ".missiond/v2/drafts/gptpro/intent-worker-v0.2.lisp (gptpro 原始归档)"
         ".missiond/v2/drafts/gptpro/intent-worker.lisp (v0.1 starter 归档)"
         ".missiond/v2/drafts/gptpro/worker-pillar-phase-A-brief.md (493 行本会话施工反馈包)"]
      :meta
        (phase-A-rounds 1)
        (gptpro-bytes "~80 KB v0.2 delivery")
        (pre-deviations-resolved 10)
        (fs-inference-corrections 3)
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
      :phase "phase-B-scan"
      :priority "low"
      :resolver-hint "Read experience_harvester.rs 全文 + grep 所有 import 判断 callers"))

  (derived-indexes
    (active_claims [])
    (unresolved_deviations [D010-partial])
    (open_issues [I001 I002 I003 I004 I005])
    (completed_phases [phase-0-warmup phase-A-design])
    (pre_deviations_upgraded_count 10)
    (decisions_count 10)
    (deviations_count 11)
    (completions_count 1))

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
