(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528023330-crates-missiond-daemon-src-engin"
  :intent "wo-20260528023330-crates-missiond-daemon-src-engin"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528023330-crates-missiond-daemon-src-engin-shard-default"
       :read_scope ["."]
       :write_scope ["."]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
