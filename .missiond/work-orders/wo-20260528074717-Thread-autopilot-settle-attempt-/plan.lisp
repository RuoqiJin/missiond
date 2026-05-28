(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528074717-Thread-autopilot-settle-attempt-"
  :intent "wo-20260528074717-Thread-autopilot-settle-attempt-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528074717-Thread-autopilot-settle-attempt--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/engine/shared_memory.rs"
                     ".missiond/work-orders/wo-20260528074717-Thread-autopilot-settle-attempt-"]
       :acceptance ["rustfmt --edition 2021 --check crates/missiond-daemon/src/engine/intent_engine/autopilot.rs crates/missiond-daemon/src/engine/shared_memory.rs"
                    "cargo test -p missiond-daemon completed_task_result_artifact -- --nocapture"
                    "git diff --check"])))
