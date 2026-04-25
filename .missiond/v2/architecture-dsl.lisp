;; ═════════════════════════════════════════════════════════════
;; MissionD — Reusable Architecture DSL (architecture-v1)
;; 目标: 把“pillar / function / flow 都有 ingress → logic-core → egress”
;;       升格为可复用、可检查、可生成图的 Lisp DSL。
;; 适用: MissionD v2 架构 Lisp, 以及未来其他程序的架构 Lisp。
;; ═════════════════════════════════════════════════════════════

(defdsl architecture-v1
  :version "v0.1"
  :status "declarative schema 2026-04-25 — reader/checker first, compiler later"
  :checker "scripts/check-architecture-lisp.mjs"

  (purpose
    "声明架构 Lisp 的通用语法骨架; 第一阶段只做 parse + shape validation, 不做代码生成")

  (reader-contract
    :syntax "S-expression + bracket vector"
    :comments "; to end-of-line"
    :strings "double-quoted strings with backslash escapes"
    :balanced-delimiters ["()" "[]"]
    :non-goal "不要求兼容 Common Lisp reader; 本 DSL 是数据语言, 不是 CL 程序")

  (node-types
    (architecture
      :desc "一个程序或系统的总架构"
      :required [ingress logic-core egress]
      :children [pillar interface dependency-map])

    (pillar
      :desc "系统的一级 ownership 单元"
      :required [pillar-ingress pillar-core pillar-egress]
      :children [function capability-family flow section module invariants])

    (capability-family
      :desc "一组对外能力或工具族"
      :required [ingress logic-core egress]
      :children [tool function flow])

    (function
      :desc "pillar 内可独立理解的功能原子/分子"
      :required [ingress logic-core egress]
      :children [step function interface invariants])

    (flow
      :desc "跨 pillar 的 ordered choreography"
      :required [ingress logic-core egress]
      :children [step branch invariant])

    (tool
      :desc "对外 endpoint 原子"
      :required [ingress logic-core egress]
      :children [step validation dispatch audit])

    (step
      :desc "有顺序的最小执行动作"
      :required [:id :owner :action]
      :optional [:reads :writes :emits :returns :calls :guards :errors]))

  (required-shape
    (pillar   [pillar-ingress pillar-core pillar-egress])
    (function [ingress logic-core egress])
    (flow     [ingress logic-core egress])
    (tool     [ingress logic-core egress]))

  (semantic-rules
    (R001 :name "single-owner"
          :rule "每个 function/flow/tool 的主 owner 必须能追溯到一个 pillar")
    (R002 :name "ordered-steps"
          :rule "logic-core 内 step id 应按 s1/s2/s3 顺序递增")
    (R003 :name "explicit-egress"
          :rule "每个 egress 至少声明 writes / emits / returns / downstream 之一")
    (R004 :name "no-hidden-cross-pillar-call"
          :rule "跨 pillar 调用必须写在 :calls / :to-* / :cross-pillar 中")
    (R005 :name "tool-flow-ref"
          :rule "每个 tool 必须能分类为 named-flow / shared-flow / trivial-single-step / pending-with-reason")
    (R006 :name "no-runtime-in-flow"
          :rule "flow 只写 choreography, runtime mechanics 必须归 worker")
    (R007 :name "no-schema-in-worker"
          :rule "worker 只能读写 memory owned schema, 不能声明 schema ownership"))

  (checker-contract
    :phase-1 ["parse all files" "balanced () and []" "unterminated string" "unexpected delimiter"]
    :phase-2 ["recursive contract files must have pillar-ingress/pillar-core/pillar-egress"
              "pillar-core function blocks must have ingress/logic-core/egress"
              "direct step sequence should be s1/s2/s3"]
    :future-phases ["load this defdsl as data"
                    "validate required-shape dynamically"
                    "emit JSON IR"
                    "generate Mermaid / Markdown / review checklist"]))
