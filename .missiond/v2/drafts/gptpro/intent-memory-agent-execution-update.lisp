;; ══════════════════════════════════════════════════════
;; MissionD — Memory Pillar Patch
;; 目标位置: intent-memory.lisp :: module board :: helper agent-execution-coordination
;; 用途: 把 execution-log 在 event-bus / memory 两次施工中暴露出的真实情况, 回灌到 pillar 设计
;; ══════════════════════════════════════════════════════

(module board
  (helper agent-execution-coordination
    (desc "并行 agent 协作的共享内存层 — 从单次 pilot 升级为可复用的正式协议")
    :status "v0.5.1 patch draft"
    :actual-evidence
      ["intent-event-bus-execution.lisp — 首个成功 pilot"
       "intent-memory-execution.lisp — 第二次复用"
       "intent-memory-execution.lisp 出现重复 D010 — 暴露手工编号缺口"]

    (actual-findings-from-pilots
      (finding-1
        :fact "execution-log 已不只是 event-bus 特例, memory pillar 施工也在使用"
        :design-impact "helper 应从 'board 的一个小技巧' 升级为 board 下的正式 operational protocol")

      (finding-2
        :fact "仅靠 '写前 claim 下一个 ID' 的口头约定不够; memory execution log 已出现重复 D010"
        :design-impact "必须把 ID 分配从人工协商改成 manager 原子分配")

      (finding-3
        :fact "当前 claim 只有占用语义, 没有 lease / heartbeat / stale-claim 回收"
        :design-impact "多 agent 长时施工必须有 claim 生命周期")

      (finding-4
        :fact "phase / completion / evidence / deviation / issue 已经成为施工刚需"
        :design-impact "execution file 不应只是自由写日志, 应有固定 slot + 衍生索引")

      (finding-5
        :fact "frozen design lisp 与 execution lisp 的 pairing 已成为事实模式"
        :design-impact "file-governance 应正式要求 :companion-log, 并允许 status/audit/repair 工具消费"))

    ;; ── 正式存储协议 ─────────────────────────────────────
    (shared-memory-slots
      (meta
        :purpose "execution 实例元数据"
        :fields "execution_id / parent_design / status / opened_at / last_updated_at / owner / scope / companion_of")

      (id-counters
        :purpose "原子 ID 分配状态"
        :fields "next-claim-id / next-deviation-id / next-decision-id / next-issue-id / next-completion-id"
        :rationale "消除 D010 这种手工重复编号问题")

      (phase-tracker
        :purpose "phase 当前状态 + cursor + owner + started/completed"
        :fields "current_phase / phases[] / stage_cursor / checkpoints[]")

      (claims
        :purpose "谁锁了什么 scope"
        :fields "claim_id / claimer / scope / phase / acquired_at / lease_expires_at / heartbeat_at / released_at / status"
        :conflict-key "scope overlap"
        :reap-rule "lease_expires_at < now 且无 heartbeat → stale-claim")

      (deviations
        :purpose "frozen design 与实际实施偏差"
        :fields "deviation_id / lisp_said / actually_found_or_did / reason / approved_by / decided_action / status / at")

      (decisions
        :purpose "执行期小决策"
        :fields "decision_id / context / options / chosen / rationale / decided_by / at")

      (completions
        :purpose "阶段完成证据"
        :fields "completion_id / phase / agent / summary / deliverables / verification / at")

      (issues
        :purpose "阻塞 / 风险 / 未决事项"
        :fields "issue_id / severity / desc / resolution_path / resolved_at / owner / at")

      (derived-indexes
        :purpose "供 status / audit 快速读取的衍生视图"
        :fields "active_claims / open_issues / unresolved_deviations / latest_decisions / completed_phases"
        :materialization "可从正文重建; 可选写回缓存"))

    ;; ── 对外接口 ────────────────────────────────────────
    (manager-interface
      :status "应该从 'Edit lisp 文件' 升级为受控 manager"
      :implementation-plan "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"

      (mcp-tool-design mission_execution
        (action open
          :args "execution_id + parent_design + scope + owner?"
          :writes "meta + id-counters 初始化 + phase-tracker 初始态")

        (action claim
          :args "execution_id + phase + claimer_name + scope + lease_secs"
          :atomicity "先查 overlap claim; 无冲突后分配 claim_id 并写入"
          :writes "claims + active_claims index")

        (action heartbeat
          :args "execution_id + claim_id + claimer_name"
          :writes "claims.heartbeat_at + lease_expires_at 延长"
          :purpose "长任务保活")

        (action release
          :args "execution_id + claim_id + claimer_name + summary?"
          :writes "claims.status=released + released_at")

        (action deviate
          :args "execution_id + lisp_said + actually_found + reason + approved_by?"
          :id-allocation "manager 自动分配 D<NNN>"
          :writes "deviations")

        (action decide
          :args "execution_id + context + options + chosen + rationale + decided_by"
          :id-allocation "manager 自动分配 DC<NNN>"
          :writes "decisions")

        (action issue
          :args "execution_id + severity + desc + resolution_path? + owner?"
          :id-allocation "manager 自动分配 I<NNN>"
          :writes "issues")

        (action complete
          :args "execution_id + phase + agent_name + summary + deliverables? + verification?"
          :id-allocation "manager 自动分配 COMP<NNN>"
          :writes "completions + phase-tracker"
          :side-effect "若 phase 全部完成可更新 meta.status")

        (action status
          :args "execution_id"
          :returns "meta + phase-tracker + active_claims + unresolved_deviations + open_issues + latest_decisions")

        (action audit
          :args "execution_id"
          :checks ["paren balance"
                   "ID 单调且无重复"
                   "claim overlap"
                   "stale claim"
                   "completion 是否覆盖每个 completed phase"
                   "open issue 是否有 owner"]
          :returns "audit report")

        (action repair
          :args "execution_id + mode(dry_run|apply)"
          :scope "仅修结构性问题: duplicate-id renumber suggestion / stale-claim mark / derived-index rebuild"
          :guard "不会 silently 改正文语义条目")))

    ;; ── 文件协议 ────────────────────────────────────────
    (file-protocol
      :current-format ".missiond/v2/<design-name>-execution.lisp"
      :required-pairing "每个 frozen / managed design lisp 都有 :companion-log"
      :write-mode "整体 read → validate → modify → write; 或 file lock"
      :rebuild-rule "derived-indexes 可删可重建, 正文 slots 才是 durable truth")

    ;; ── 设计约束 ────────────────────────────────────────
    (invariants
      :inv-1 "ID 只能由 manager 分配, 不允许人工手写下一个编号"
      :inv-2 "claim 不是永久锁, 必须带 lease + heartbeat + reap"
      :inv-3 "所有 write 必须做 paren-balance 与 schema-shape 校验"
      :inv-4 "status 读取优先走 derived-indexes, 但 audit 以正文 slot 为真"
      :inv-5 "deviation 不等于立即改 frozen design; 仍需指挥官批准"
      :inv-6 "execution = operational state; methodology = reusable playbook; 二者不得混写")

    (placement-rationale
      :why-board
        ["board 是任务编排中心"
         "execution 实例本质是一次多 agent 协作任务"
         "phase / claim / completion 天然可与 board_task 或 flow_context 联动"]
      :why-not-intent-layer
        ["intent-layer 管 prescriptive 规约: 应该怎么做"
         "execution log 管 operational 实况: 这次实际怎么做"])))
