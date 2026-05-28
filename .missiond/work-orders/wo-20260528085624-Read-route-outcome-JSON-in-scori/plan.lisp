(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528085624-Read-route-outcome-JSON-in-scori"
  :intent "wo-20260528085624-Read-route-outcome-JSON-in-scori"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528085624-Read-route-outcome-JSON-in-scori-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/shared_memory.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528085624-Read-route-outcome-JSON-in-scori/intent.lisp"
                     ".missiond/work-orders/wo-20260528085624-Read-route-outcome-JSON-in-scori/plan.lisp"
                     ".missiond/work-orders/wo-20260528085624-Read-route-outcome-JSON-in-scori/audit.lisp"]
       :acceptance ["cargo test -p missiond-daemon feature_gates::tests::non_core_tools_are_feature_gated"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "git diff --check"])))
