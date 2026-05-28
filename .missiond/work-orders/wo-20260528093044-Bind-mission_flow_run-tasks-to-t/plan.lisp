(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528093044-Bind-mission_flow_run-tasks-to-t"
  :intent "wo-20260528093044-Bind-mission_flow_run-tasks-to-t"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528093044-Bind-mission_flow_run-tasks-to-t-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/compute/flow_run.rs" "scripts/check-v3-compute-primitives-isomorphism.mjs"]
       :acceptance ["cargo fmt --check -p missiond-daemon" "node scripts/check-v3-compute-primitives-isomorphism.mjs --json"])))
