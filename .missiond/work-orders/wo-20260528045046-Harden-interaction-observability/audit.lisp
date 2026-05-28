(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260528045046-Harden-interaction-observability"
  :events ((event created :at "2026-05-28T04:50:46Z" :actor codex)
           (event scope-declared
             :at "2026-05-28T04:50:46Z"
             :summary "Service/dev interaction permission contexts must not emit null user_id; conversation observability reads cast message_count for stable sqlx decoding.")))
