(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260602181820-Route-MissionD-authority-evidenc"
  :events ((event created :at "2026-06-02T18:18:20.515Z" :actor missiond-work-order)
           (event live-smoke-finding
             :at "2026-06-02T18:30:24Z"
             :actor codex
             :summary "mission_memory evidence_backfill source=projects crashed missiond with stack overflow at handle_provider_evidence_backfill because project/support authority backfill awaited a large context_gather future shape through the shared local_evidence_backfill_response async state machine.")
           (event fix
             :at "2026-06-02T18:35:00Z"
             :actor codex
             :summary "Box local_evidence_backfill_response and context_gather::handle futures so compiled authority prewarm stays stack-isolated from skill/context-gather branches; V3 SSOT and checker now require this runtime-stack guard.")
           (event search-quality-fix
             :at "2026-06-02T18:48:00Z"
             :actor codex
             :summary "mission_memory evidence_search now dedupes repeated context_gather/backfill projections by lane/source/project, and compiled authority backfill emits compact service/support evidence_refs instead of wide runtime payloads.")))
