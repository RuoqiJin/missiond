(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528074031-Require-write-scoped-settle-atte"
  :intent "wo-20260528074031-Require-write-scoped-settle-atte"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528074031-Require-write-scoped-settle-atte-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/shared_memory.rs"
                     ".missiond/work-orders/wo-20260528074031-Require-write-scoped-settle-atte"]
       :acceptance ["rustfmt --edition 2021 --check crates/missiond-daemon/src/engine/shared_memory.rs"
                    "cargo test -p missiond-daemon write_scoped_settle_attempt -- --nocapture"
                    "git diff --check"])))
