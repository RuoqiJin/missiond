(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260602200816-deployment-event-context-lane"
  :events ((event created
             :at "2026-06-02T20:08:16Z"
             :actor missiond-work-order)
           (event live-benchmark
             :at "2026-06-02T20:08:16Z"
             :actor codex
             :summary "Payments deploy_ops retrieval is scoped and clean, but Deploy Center release/agent/canary runtime evidence is still represented mostly by support_catalog fallback; add a bounded EventBridge deployment_events source that reads only system::external_service_event and filters before injection.")
           (event verification
             :at "2026-06-02T20:14:07Z"
             :actor codex
             :summary "Implemented deployment_events runtime_truth source, unit-tested scoped Deploy Center event filtering, regenerated V3 projections, and verified cargo context_gather tests, cargo check, memory KB isomorphism, runtime path hygiene, source hygiene, deployment closure plane, project contracts, and diff whitespace.")))
