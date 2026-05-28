(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528093510-Bind-run_methodology-tasks-to-ta"
  :intent "wo-20260528093510-Bind-run_methodology-tasks-to-ta"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528093510-Bind-run_methodology-tasks-to-ta-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/knowledge/workflow/run_methodology.rs" "scripts/check-v3-workflow-isomorphism.mjs"]
       :acceptance ["cargo fmt --check -p missiond-daemon" "node scripts/check-v3-workflow-isomorphism.mjs --json"])))
