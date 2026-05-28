(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528073830-Pin-grounded-dispatch-kernel-art"
  :intent "wo-20260528073830-Pin-grounded-dispatch-kernel-art"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528073830-Pin-grounded-dispatch-kernel-art-shard-default"
       :read_scope ["."]
       :write_scope ["scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528073830-Pin-grounded-dispatch-kernel-art"]
       :acceptance ["node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "git diff --check"])))
