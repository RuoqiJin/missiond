(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528024612-.missiond-v3-runtime-compiled-co"
  :intent "wo-20260528024612-.missiond-v3-runtime-compiled-co"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528024612-.missiond-v3-runtime-compiled-co-shard-default"
       :read_scope ["."]
       :write_scope ["."]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
