(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528075332-Guard-BoardTask-claim-capability"
  :intent "wo-20260528075332-Guard-BoardTask-claim-capability"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528075332-Guard-BoardTask-claim-capability-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/knowledge/board/claim.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     ".missiond/work-orders/wo-20260528075332-Guard-BoardTask-claim-capability"]
       :acceptance ["rustfmt --edition 2021 --check crates/missiond-daemon/src/handlers/knowledge/board/claim.rs crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
