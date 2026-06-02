(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260603035200-context-evidence-freshness"
  :events ((event created :at "2026-06-02T19:40:32.662Z" :actor missiond-work-order)
           (event persisted-compiled-policy-freshness-filter
             :at "2026-06-02T19:58:00Z"
             :actor codex
             :summary "stale external compiled runtime poisoning test showed active support_catalog used release-local policy, but postgres.evidence_items could still return old compiled-deployment-policy path/source_hash refs; context_gather now filters those persisted rows before response/context injection and reports freshness_filtered_count.")))
