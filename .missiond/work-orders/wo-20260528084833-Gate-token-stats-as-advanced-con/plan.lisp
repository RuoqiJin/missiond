(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528084833-Gate-token-stats-as-advanced-con"
  :intent "wo-20260528084833-Gate-token-stats-as-advanced-con"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528084833-Gate-token-stats-as-advanced-con-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/feature_gates.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528084833-Gate-token-stats-as-advanced-con/intent.lisp"
                     ".missiond/work-orders/wo-20260528084833-Gate-token-stats-as-advanced-con/plan.lisp"
                     ".missiond/work-orders/wo-20260528084833-Gate-token-stats-as-advanced-con/audit.lisp"]
       :acceptance ["cargo test -p missiond-daemon feature_gates::tests::non_core_tools_are_feature_gated"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "git diff --check"])))
