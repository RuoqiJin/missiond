(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528092523-Return-shared-memory-lease-confl"
  :intent "wo-20260528092523-Return-shared-memory-lease-confl"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528092523-Return-shared-memory-lease-confl-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/shared_memory.rs" "scripts/check-v3-control-plane-kernel-isomorphism.mjs"]
       :acceptance ["cargo fmt --check -p missiond-daemon" "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"])))
