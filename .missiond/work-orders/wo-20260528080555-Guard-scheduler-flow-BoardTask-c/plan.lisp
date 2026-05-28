(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528080555-Guard-scheduler-flow-BoardTask-c"
  :intent "wo-20260528080555-Guard-scheduler-flow-BoardTask-c"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528080555-Guard-scheduler-flow-BoardTask-c-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
                     ".missiond/work-orders/wo-20260528080555-Guard-scheduler-flow-BoardTask-c"]
       :acceptance ["rustfmt --edition 2021 --check crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
