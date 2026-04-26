;; ═════════════════════════════════════════════════════════════
;; MissionD — Capability Governance Shard (L2 split, wave 15 task 02)
;; 目标: capability-evolution-governance + capability_usage handler 周边的
;;       semantic evidence v1 + 5 sources + lisp hint merge-candidate +
;;       capability-usage-read-model + workflow stats / DispatcherEvent /
;;       WorkerEvent 聚合
;; 来源: 由 wave 15 task 02 从 intent-intent-layer.lisp + intent-flow.lisp +
;;       intent-tools.lisp + intent-memory.lisp 物理 split 出独立 shard;
;;       内容与原主 Lisp byte-identical (no-content-mutation, R5)
;; 父引用: 4 父文件各留 stub 指回本 shard
;; 拆分计划: architecture-dsl.lisp :: l2-shard-split-plan ::
;;           shard intent-capability-governance
;; ═════════════════════════════════════════════════════════════

(capability-governance-shard
  :version "v1"
  :origin "wave 15 task 02 — physical split from intent-layer/flow/tools/memory parents"
  :status "code-aligned partial — semantic evidence v1 已落 (commit 3ed14fc)"
  :stability "section-ids preserved (R008 + R016); content byte-identical to pre-split parents"

  ;; ──────────────────────────────────────────────────────────
  ;; A · intent-layer pillar :: section capability-evolution-governance
  ;; (was: intent-intent-layer.lisp lines 758–807)
  ;; section-id roots:
  ;;   intent-layer.section.capability-evolution-governance
  ;;   intent-layer.capability-evolution-governance.semantic-evidence-v1
  ;; ──────────────────────────────────────────────────────────
  (section capability-evolution-governance
    :desc "能力使用度监控只给证据; 本 pillar 决定保留、合并、废弃、删除"
    :status "policy-designed + mission_capability_usage semantic evidence v1 code-aligned partial — 5 sources (conversation_tool_calls / board_tasks_flow_template / event_log probe / lisp_semantic_hints / review_sidecar) + merge-candidate 由 lisp hints (replacement/moved-to/preferred/supersedes/consolidated) 保守生成; 自动 deprecate/remove 仍由人工/Lisp 主导 (anchor: intent-layer.capability-evolution-governance.semantic-evidence-v1)"
    :flow-ref "flow pillar :: F-capability-usage-monitoring"
    :memory-ref "memory pillar :: system-support :: capability-usage-read-model"
    :tool-ref "tools pillar :: mission_capability_usage"
    :implementation-targets ["crates/missiond-mcp/src/tools/comm/capability_usage.rs (schema + replacement_target field)"
                             "crates/missiond-daemon/src/handlers/comm/capability_usage.rs (HintIndex 解析 .missiond/v2/intent-{tools,flow,intent-layer}.lisp + probe_event_log_flows + ReviewState sidecar + non-destructive mark/ack/merge)"]
    :hint-conventions
      ["intent-tools.lisp / intent-flow.lisp / intent-intent-layer.lisp 使用 :replacement \"mission_…\" / :moved-to \"F-…\" / :supersedes \"…\" / :status \"deprecated-…\" 标记 → 自动进入 merge-candidate"
       "consolidated-keep token 显式压制 merge-candidate"
       "merge 仅在 target 解析为已注册 tool/flow 且 target != source 且 target 非 protected 时给出"]
    :pending ["semantic merge 自动执行 (即使 hint 完整, 也只产候选, 不动 registry)"
              "workflow stats / success rate 进入 read-model"
              "DispatcherEvent / WorkerEvent / ExecutionEvent 等 event source 聚合"]

    (policy
      :never-auto-delete true
      :human-approval-required ["deprecate" "merge" "remove" "schema-breaking replacement"]
      :protected-capabilities ["daemon bootstrap" "memory/event-bus repair" "mission_execution" "mission_intent" "mission_forge_*" "manual recovery tools"]
      :quiet-threshold "no calls in 30d, but not enough for removal"
      :stale-threshold "no calls in 90d or never-used after introduction grace window"
      :replacement-required-for-merge "low usage alone only marks stale; merge/deprecate needs explicit better implementation or semantic overlap evidence")

    (lifecycle-states
      (active             :meaning "recently used or structurally required")
      (quiet              :meaning "low/no recent usage; keep visible, monitor")
      (stale-candidate    :meaning "长期未用; 进入 review report")
      (shadowed-candidate :meaning "存在更好实现或 consolidated dispatcher 覆盖")
      (merge-candidate    :meaning "语义重叠,可把 caller 或 docs 收敛到 target capability")
      (deprecated         :meaning "人工批准后保留兼容窗口,返回替代建议")
      (removed            :meaning "兼容窗口结束且代码/Forge/schema 同步清理完成"))

    (path capability-lifecycle-review
      :lifecycle-style periodic-governance
      (ingress
        :source "F-capability-usage-monitoring report / mission_capability_usage(action=report|candidates)"
        :entry-components ["usage snapshot" "candidate list" "replacement evidence" "protected capability list"])
      (logic-core
        (step s1 "validate-evidence: check count windows, last_used_at, success/failure, caller diversity, and age since introduction")
        (step s2 "apply-protection: mark critical bootstrap/repair/manual recovery surfaces as protected even if rarely used")
        (step s3 "decide-class: assign active/quiet/stale/shadowed/merge/deprecated/remove proposal")
        (step s4 "require-replacement: merge/deprecate only if target capability is named and has same-or-better flow coverage")
        (step s5 "write-decision: record rationale in architecture Lisp or capability-usage-review.json sidecar; never rely on raw count alone")
        (step s6 "schedule-code-alignment: approved decisions become PLAN.lisp / mission_execution tasks / Forge registry changes")
        (step s7 "post-change-monitor: keep deprecated alias under observation through compatibility window"))
      (egress
        :writes ["architecture decision note" "optional board task / PLAN.lisp" "future workflow distillation evidence"]
        :reads ["capability usage snapshot" "intent-tools registry" "intent-flow index" "memory/tool audit history"]
        :returns "keep / monitor / merge / deprecate / remove-after-compat-window decision")))
  ;; ──────────────────────────────────────────────────────────
  ;; B · flow pillar :: F-capability-usage-monitoring
  ;; (was: intent-flow.lisp lines 863–893)
  ;; section-id root: flow.capability-usage-monitoring
  ;; Wrapped in (category-wrapper ...) to absorb the extra trailing `)`
  ;; that originally closed the parent (category workflow-runtime-flows ...).
  ;; ──────────────────────────────────────────────────────────
  (category-wrapper-flow-capability-usage
    (flow F-capability-usage-monitoring
      :desc "tool / flow 调用热度监控 → 落灰/替代/合并候选 → 人工治理决策"
      :status "code-aligned partial — semantic evidence v1: 5 sources (conversation_tool_calls / board_tasks_flow_template / event_log_flow_events probe / lisp_semantic_hints / review_sidecar) + merge-candidate 由 lisp hints 保守生成 + ObservabilityEvent emit; semantic merge 自动决策 / event_log 全量聚合 / workflow stats 仍 pending (anchor: intent-layer.capability-evolution-governance.semantic-evidence-v1)"
      :triggers
        ["periodic daily/weekly monitor tick"
         "mission_capability_usage(action=snapshot|report|candidates|mark|ack)"
         "manual architecture cleanup review before removing/merging tools or flows"]
      :ingress
        (entry "monitor request with window={7d|30d|90d|all}, scope={tool|flow|both}, project_id?, include_protected?")
      :implementation-targets
        ["crates/missiond-mcp/src/tools/comm/capability_usage.rs (schema + replacement_target field)"
         "crates/missiond-daemon/src/handlers/comm/capability_usage.rs (5-source coverage + HintIndex 解析 .missiond/v2/intent-{tools,flow,intent-layer}.lisp + probe_event_log_flows + ReviewState sidecar)"]
      (logic-core
        (step s1 "collect-tool-usage: read memory :: system-support :: capability-usage-read-model over conversation_tool_calls")
        (step s2 "collect-flow-usage: board_tasks.flow_template + YAML flow registry + event_log_flow_events read-only probe (board::task_created since 90d, payload.flow_template 抽取); workflow stats 仍 deferred")
        (step s3 "normalize-capabilities: map MCP registry ids, deprecated names, consolidated dispatchers, and tool-backed flow refs into canonical capability ids")
        (step s4 "join-architecture-index: HintIndex 扫 intent-tools.lisp + intent-flow.lisp + intent-intent-layer.lisp, 解析 :replacement / :moved-to / :supersedes / :status \"deprecated-*\" / consolidated-keep")
        (step s5 "classify-usage: active / quiet / stale / never-used / protected / shadowed-by-better-capability / merge-candidate (merge-candidate 仅在 hint token ∈ {deprecated, moved-to, replacement, supersedes} 且 target 解析为已注册 tool/flow 时给出; consolidated-keep token 显式压制 merge)")
        (step s6 "score-candidates: combine count, recency, success rate, criticality, age since introduction, and explicit replacement link")
        (step s7 "emit-review-report: produce evidence-first report; do not delete, hide, or mutate tools/flows")
        (step s8 "governance-decision: intent-layer may mark keep / consolidate / deprecate / remove-after-compat-window")
        (step s9 "follow-up-implementation: approved changes become Lisp edits first, then Forge/ClaudeCode code-alignment tasks"))
      :egress
        (writes [".missiond/v2/capability-usage-review.json sidecar (mark/ack/merge with replacement_target)" "ObservabilityEvent::CapabilityUsageSnapshot / CapabilityStaleCandidate" "future review note / board task"])
        (reads ["conversation_tool_calls" "board_tasks.flow_template" "event_log (board::task_created)" "MCP tool registry" "YAML flow registry" "intent-tools/intent-flow/intent-intent-layer lisp hints"])
        (returns "capability usage snapshot + candidate governance actions with evidence (sources[5])")
        (downstream ["intent-layer :: capability-evolution-governance" "tools :: mission_capability_usage" "memory :: capability-usage-read-model"]))
      :pending ["semantic merge 自动决策 (即使 hint 完整, 也只产候选, 不动 registry)"
                "workflow stats / success rate 进 read-model"
                "event_log 之外的事件 source (DispatcherEvent / WorkerEvent / ExecutionEvent) 聚合"]
      :tools-backref ["mission_audit" "mission_timeline" "mission_codex_ops" "mission_capability_usage"])
  ;; ──────────────────────────────────────────────────────────
  ;; C · tools pillar :: implemented-surface mission_capability_usage
  ;; (was: intent-tools.lisp lines 2930–2960)
  ;; section-id root: tools.surface.mission-capability-usage
  ;; Wrapped to absorb the extra trailing `)` that originally closed the
  ;; parent (section mcp-surface-lifecycle ...).
  ;; ──────────────────────────────────────────────────────────
  (section-wrapper-mission-capability-usage
    (implemented-surface mission_capability_usage
      :status "code-aligned partial — semantic evidence v1: 5 sources (conversation_tool_calls / board_tasks_flow_template / event_log_flow_events probe / lisp_semantic_hints / review_sidecar) + merge-candidate 由 lisp hints (replacement/moved-to/preferred/supersedes/consolidated) 保守生成 + replacement_target 字段 + non-destructive mark/ack/merge; semantic merge 自动决策 / DispatcherEvent/WorkerEvent/ExecutionEvent 聚合 / workflow stats 仍 pending (anchor: intent-layer.capability-evolution-governance.semantic-evidence-v1)"
      :actions ["snapshot" "report" "candidates" "mark" "ack"]
      :owner-boundary "tools owns MCP schema; memory owns read-model; flow owns monitoring choreography; intent-layer owns lifecycle decision"
      :code ["crates/missiond-mcp/src/tools/comm/capability_usage.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage.rs"]
      :implementation-targets ["crates/missiond-mcp/src/tools/comm/capability_usage.rs (schema + replacement_target field for merge)"
                               "crates/missiond-daemon/src/handlers/comm/capability_usage.rs (HintIndex 解析 .missiond/v2/intent-{tools,flow,intent-layer}.lisp + probe_event_log_flows + ReviewState sidecar + protected source/target rejection)"]
      :semantic-evidence-v1
        ["sources[5]: conversation_tool_calls / board_tasks_flow_template / event_log_flow_events (board::task_created since 90d) / lisp_semantic_hints (扫 3 文件) / review_sidecar"
         "merge-candidate: 仅在 hint token ∈ {deprecated, moved-to, replacement, supersedes} 且 target 解析为已注册 tool/flow 且 target ≠ source 且 target 非 protected 时给出"
         "consolidated-keep token 显式压制 merge-candidate"
         "mark/ack/merge non-destructive — 仅写 .missiond/v2/capability-usage-review.json sidecar, 不动 registry"]
      (ingress
        :schema "action required; window? default 30d; scope=tool|flow|both; project?; candidate_id? for mark/ack; replacement_target? optional for merge decisions; dry_run optional for mark"
        :callers ["architecture cleanup review" "intent-layer governance" "external MCP client"])
      (logic-core
        (step s1 "validate action/window/scope and refuse destructive action; mark/ack only records review status")
        (step s2 "snapshot/report/candidates call F-capability-usage-monitoring; 5 sources 联合查询")
        (step s3 "candidates returns evidence-ranked active/quiet/stale/never-used/shadowed/protected/merge-candidate buckets")
        (step s4 "mark records human decision intent to sidecar; merge 必须 replacement_target 显式或 hint target 解析得到, 否则拒绝")
        (step s5 "ack closes a report item only after follow_up_ref points to PLAN.lisp / mission_execution / board task"))
      (egress
        :writes [".missiond/v2/capability-usage-review.json sidecar (mark/ack/merge with replacement_target)" "no daemon_state JSON cache because daemon_state is i64-only"]
        :reads ["conversation_tool_calls" "board_tasks.flow_template" "event_log (board::task_created)" "MCP registry" "YAML flow registry" "intent-tools/intent-flow/intent-intent-layer lisp hints" "review sidecar"]
        :flow-ref "F-capability-usage-monitoring (code-aligned partial — semantic evidence v1)"
        :event-bus "ObservabilityEvent::CapabilityUsageSnapshot / CapabilityStaleCandidate emitted after snapshot/candidates computation"
        :returns "usage snapshot (含 source_coverage.sources[5]) / candidate report / review receipt")
      :pending ["semantic merge 自动决策 (即使 hint 完整, 也只产候选, 不动 registry)"
                "DispatcherEvent / WorkerEvent / ExecutionEvent 等 event source 聚合"
                "workflow stats / success rate 进 read-model"]))
  ;; ──────────────────────────────────────────────────────────
  ;; D · memory pillar :: derived-read-model capability-usage-read-model
  ;; (was: intent-memory.lisp lines 2309–2359)
  ;; section-id root: memory.system-support.capability-usage-read-model
  ;; ──────────────────────────────────────────────────────────
    (derived-read-model capability-usage-read-model
      :status "code-aligned v0.5.6 — semantic evidence v1 已落 (commit 3ed14fc): 5 sources + lisp hint merge-candidate + replacement_target 字段; semantic merge 自动决策 / event_log 之外 source 聚合 / workflow stats 仍 pending"
      :owner-module "system-support"
      :storage-policy "当前实现按需聚合,不缓存 snapshot: daemon_state trait 只支持 i64,不能存 JSON; mark/ack/merge 治理决定写 <project_root>/.missiond/v2/capability-usage-review.json sidecar (含 replacement_target/ack_at/ack_by/follow_up_ref)"
      :implementation-targets ["crates/missiond-mcp/src/tools/comm/capability_usage.rs (schema + replacement_target)"
                               "crates/missiond-daemon/src/handlers/comm/capability_usage.rs (HintIndex 解析 .missiond/v2/intent-{tools,flow,intent-layer}.lisp + probe_event_log_flows + ReviewState sidecar + protected source/target rejection)"]
      :truth-sources
        ((tool-calls
           :status "implemented"
           :reads ["conversation_tool_calls via ConversationStore::get_tool_call_global_stats(since_iso)"]
           :grain "tool_name + action + project_id? + caller + outcome + started_at/completed_at"
           :purpose "统计每个 MCP tool/action 的调用次数、最近调用、成功率、调用来源")
         (flow-runs
           :status "implemented"
           :reads ["board_tasks.flow_template" "engine::flow::loader::list_flows() YAML registry (合并 generated + core)"]
           :grain "flow_id/flow_template + node/action + project_id? + terminal status"
           :purpose "统计 named flow 与 executable workflow 的启动、完成、失败、最近使用"
           :pending ["workflow execution stats / success rate" "flow_context deep evidence"])
         (event-log-flow-events
           :status "implemented (read-only probe)"
           :reads ["event_log via query_timeline_filtered(\"board::task_created\", since=90d, limit=500); 抽 payload.flow_template / .template / .task.flow_template"]
           :grain "flow_template + event count + first/last seen"
           :purpose "补充 board_tasks 之外的 flow 事件证据 (read-only, 不写)"
           :pending ["DispatcherEvent / WorkerEvent / ExecutionEvent 等其他 event source 聚合"])
         (lisp-semantic-hints
           :status "implemented"
           :reads [".missiond/v2/intent-tools.lisp" ".missiond/v2/intent-flow.lisp" ".missiond/v2/intent-intent-layer.lisp"]
           :purpose "扫描 :replacement \"mission_…\" / :moved-to \"F-…\" / :supersedes \"…\" / :status \"deprecated-…\" / consolidated-keep 标记, 形成 HintIndex"
           :merge-candidate-rule "仅在 hint token ∈ {deprecated, moved-to, replacement, supersedes} 且 target 解析为已注册 tool/flow 且 target ≠ source 且 target 非 protected 时给出; consolidated-keep 显式压制")
         (review-sidecar
           :status "implemented"
           :reads [".missiond/v2/capability-usage-review.json"]
           :purpose "读出 mark/ack/merge 历史决策 + replacement_target 关联"))
      (ingress
        :source "mission_capability_usage(action=snapshot|report|candidates|mark|ack); future periodic monitor tick"
        :entry-components ["tools audit log" "event-bus event_log (board::task_created)" "board/workflow state" ".missiond/v2/*.lisp hints" "intent-layer governance rules"])
      (logic-core
        (step s1 "collect tool usage by window: 7d / 30d / 90d / all-time from conversation_tool_calls")
        (step s2 "collect flow usage from board_tasks.flow_template + event_log probe + YAML flow registry (generated + core)")
        (step s3 "build HintIndex 扫 3 lisp 文件; normalize against MCP registry; protected source/target 在治理规则里")
        (step s4 "apply protected capability patterns from intent-layer governance")
        (step s5 "emit categories: active / quiet / stale / never-used / shadowed-by-better-capability / merge-candidate / protected (merge-candidate 仅 hint-driven)")
        (step s6 "return source_coverage.sources[5] with read-only persistence note; never mutate registry or Lisp directly")
        (step s7 "mark/ack/merge write review sidecar only; merge 必须 replacement_target 显式或 hint 解析得到, target protected 拒绝"))
      (egress
        :writes ["<project_root>/.missiond/v2/capability-usage-review.json for mark/ack/merge only" "ObservabilityEvent::CapabilityUsageSnapshot / CapabilityStaleCandidate"]
        :reads ["conversation_tool_calls" "board_tasks.flow_template" "event_log (board::task_created)" "MCP registry" "YAML flow registry (generated + core)" ".missiond/v2/intent-{tools,flow,intent-layer}.lisp hints" "review sidecar"]
        :returns "capability usage snapshot (含 source_coverage.sources[5]) + stale/merge/deprecate candidates with evidence")
      :pending ["semantic merge 自动决策 (即使 hint 完整, 也只产候选, 不动 registry)"
                "DispatcherEvent / WorkerEvent / ExecutionEvent 等其他 event source 聚合"
                "workflow execution stats / success rate"]))
