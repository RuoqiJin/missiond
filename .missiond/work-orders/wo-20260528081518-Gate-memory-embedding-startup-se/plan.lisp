(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528081518-Gate-memory-embedding-startup-se"
  :intent "wo-20260528081518-Gate-memory-embedding-startup-se"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528081518-Gate-memory-embedding-startup-se-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/feature_gates.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528081518-Gate-memory-embedding-startup-se/intent.lisp"
                     ".missiond/work-orders/wo-20260528081518-Gate-memory-embedding-startup-se/plan.lisp"
                     ".missiond/work-orders/wo-20260528081518-Gate-memory-embedding-startup-se/audit.lisp"]
       :acceptance ["cargo check -p missiond-daemon"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "git diff --check"])))
