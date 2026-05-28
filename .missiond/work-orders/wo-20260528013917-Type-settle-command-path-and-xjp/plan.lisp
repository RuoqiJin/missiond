(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528013917-Type-settle-command-path-and-xjp"
  :intent "wo-20260528013917-Type-settle-command-path-and-xjp"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528013917-Type-settle-command-path-and-xjp-shard-default"
       :read_scope ["."]
       :write_scope ["."]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
