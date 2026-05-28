(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528092054-Pin-PG-lease-expiry-and-artifact"
  :intent "wo-20260528092054-Pin-PG-lease-expiry-and-artifact"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528092054-Pin-PG-lease-expiry-and-artifact-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/tests/pg_integration.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"]
       :acceptance ["cargo fmt --check -p missiond-core"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "cargo test -p missiond-core --test pg_integration --features postgres --no-run"])))
