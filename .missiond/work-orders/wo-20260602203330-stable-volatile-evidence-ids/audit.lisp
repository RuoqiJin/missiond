(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260602203330-stable-volatile-evidence-ids"
  :events ((event created
             :at "2026-06-02T20:33:30Z"
             :actor missiond-work-order)
           (event live-benchmark
             :at "2026-06-02T20:33:30Z"
             :actor codex
             :summary "After runtime freshness filters, live intent_default search still showed multiple stale runtime_environment rows being filtered. Future deploys should overwrite volatile runtime/support compact projections by stable identity instead of minting content-hash IDs for each release path/hash.")
           (event verification
             :at "2026-06-02T20:37:25Z"
             :actor codex
             :summary "Implemented stable evidence_item IDs for volatile compact projections runtime_environment, support_catalog, and deployment_closure_policy. Verified with rustfmt, targeted context_gather tests, cargo check -p missiond-daemon, V3 contract check, memory/KB isomorphism checker including dry fixture, runtime path hygiene, source hygiene, deployment closure plane checker, and git diff --check.")))
