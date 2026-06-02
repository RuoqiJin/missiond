(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260602195540-deploy-closure-evidence-hygiene"
  :events ((event created
             :at "2026-06-02T19:55:40Z"
             :actor missiond-work-order)
           (event live-benchmark
             :at "2026-06-02T19:55:40Z"
             :actor codex
             :summary "Payments deploy_ops context returned correct release-local support refs, but postgres.evidence_items also surfaced a generic service deployment closure placeholder from an older compact context_gather projection; filter incomplete placeholders before worker injection.")
           (event verified
             :at "2026-06-02T20:12:00Z"
             :actor codex
             :summary "Added incomplete_filtered_count read-model metric, unit coverage for placeholder filtering, V3 checker needles, and regenerated contract/runtime defaults; targeted context_gather tests and V3 memory/check/hygiene commands passed.")))
