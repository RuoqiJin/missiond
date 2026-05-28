(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528100346-Bind-strategy-and-master-control"
  :intent "wo-20260528100346-Bind-strategy-and-master-control"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528100346-Bind-strategy-and-master-control-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/workers/gemini/strategy_worker.rs"
                     "crates/missiond-daemon/src/engine/master_control.rs"
                     "scripts/check-v3-master-control-isomorphism.mjs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"]
       :acceptance ["cargo fmt --check -p missiond-daemon"
                    "node scripts/check-v3-master-control-isomorphism.mjs --json"
                    "node scripts/check-v3-memory-kb-isomorphism.mjs --json"])))
