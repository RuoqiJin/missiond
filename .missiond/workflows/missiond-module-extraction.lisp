(workflow missiond-module-extraction
  :schema "missiond.workflow.v1"
  :workflow_id missiond-module-extraction
  :status active
  :source_plans [memory-provider-contract eventhub-service-contract control-plane-m6-split]
  :objective "Extract private memory and cross-service event storage from MissionD Core through provider/service contracts without breaking local orchestration."
  :match_rules
    ((trigger :kind BoardTask :title_contains "Memory Provider")
     (trigger :kind BoardTask :title_contains "EventHub")
     (trigger :kind BoardTask :title_contains "MissionD module extraction"))
  :inputs [missiond-blueprint memory-provider-contract eventhub-service-contract project-registry service-runtime-universe]
  :steps
    ((step s1 :id review-boundary
       :question "请审视 MissionD 当前职责：哪些能力必须留在本地 orchestrator，哪些应该通过 provider/service 抽离？"
       :output [boundary-review])
     (step s2 :id pin-provider-contract
       :logic "Update V3 memory-provider-contract before runtime edits. Memory must be usable as null/local/xjp provider, and every query/write must resolve tenant/universe/project/user scope."
       :output [memory-provider-contract-patch])
     (step s3 :id pin-eventhub-contract
       :logic "Update V3 eventhub-service-contract before runtime edits. MissionD local EventBus remains offline-safe; xjp-eventhub owns cross-service durable streams/waits/subscriptions/replay."
       :output [eventhub-service-contract-patch])
     (step s4 :id add-compatibility-facades
       :logic "Keep current mission_kb_* and eventbridge tools as compatibility leaves. New primary surfaces are mission_memory and eventhub/local-spool adapters."
       :output [facade-shard])
     (step s5 :id scaffold-services
       :logic "Create or update xjp-memory and xjp-eventhub project SSOT before moving data or event write paths."
       :output [service-ssot-shards])
     (step s6 :id dual-read-write
       :logic "Run compatibility migration with dual-read/double-write reports before removing MissionD local tables or event paths."
       :output [parity-report])
     (step s7 :id cutover
       :logic "Switch default reads only after provider/eventhub parity is proven and MissionD local offline behavior is preserved."
       :output [cutover-report]))
  :acceptance
    ["node scripts/check-v3-service-extraction-isomorphism.mjs --json"
     "node scripts/check-v3-code-isomorphism-complete.mjs --json"
     "node scripts/check-v3-final-convergence.mjs --json --static-only"]
  :safety
    ((rule "Do not physical-delete existing KB/conversation/event tables in the first module extraction pass.")
     (rule "Do not make xjp-eventhub required for local Board/workstation/autopilot execution.")
     (rule "Do not put secrets or provider tokens into Lisp; use secret-store/env refs only."))
  :risk-gates
    ((gate g1 :id no-data-loss :rule "First extraction pass must keep MissionD-local memory/event tables intact; xjp-memory and xjp-eventhub are dual-write peers until parity is proven.")
     (gate g2 :id offline-orchestration :rule "MissionD Core MUST continue to dispatch agents/Board/slot/workflow events with xjp-eventhub offline; cross-service relay failures are diagnostics, not block-on-error.")
     (gate g3 :id contract-first :rule "No provider/service code may land before V3 memory-provider-contract and eventhub-service-contract pin the surface and v2-convergence-map covers it."))
  :completion
    ((criterion c1 :rule "memory-provider-contract and eventhub-service-contract are pinned in V3 and covered by code-aligned v2-items.")
     (criterion c2 :rule "scripts/check-v3-service-extraction-isomorphism.mjs is green and registered in check-v3-code-isomorphism-complete.mjs PER_SURFACE_CHECKERS.")
     (criterion c3 :rule "MissionD local memory and EventBus retain offline-safe orchestration; provider/eventhub adapters are compatibility leaves until parity is proven.")
     (criterion c4 :rule "Workflow egress reports are attached to the parent BoardTask before cutover steps can advance."))
  :egress [service-extraction-boundary-report memory-provider-parity-report eventhub-spool-parity-report])
