(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260602201823-runtime-evidence-freshness"
  :events ((event created
             :at "2026-06-02T20:18:23Z"
             :actor missiond-work-order)
           (event live-benchmark
             :at "2026-06-02T20:18:23Z"
             :actor codex
             :summary "Post-deploy intent_default smoke showed live runtime_environment source summary on release 11c95088, but persisted evidence_items still recalled an older d06bd5f runtime_environment compact projection. Filter runtime_environment compiled_runtime_dir against active MISSIOND_COMPILED_RUNTIME_DIR before returning or injecting persisted evidence.")
           (event verification
             :at "2026-06-02T20:22:27Z"
             :actor codex
             :summary "Implemented runtime_environment compiled_runtime_dir freshness filtering for persisted evidence_items, added regression coverage, regenerated V3 projections, and verified cargo context_gather tests, cargo check, memory KB isomorphism normal/dry-fixture, runtime path hygiene, source hygiene, deployment closure plane, project contracts, and diff whitespace.")))
