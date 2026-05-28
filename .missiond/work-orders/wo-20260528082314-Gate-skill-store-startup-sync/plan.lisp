(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528082314-Gate-skill-store-startup-sync"
  :intent "wo-20260528082314-Gate-skill-store-startup-sync"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528082314-Gate-skill-store-startup-sync-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/main.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528082314-Gate-skill-store-startup-sync/intent.lisp"
                     ".missiond/work-orders/wo-20260528082314-Gate-skill-store-startup-sync/plan.lisp"
                     ".missiond/work-orders/wo-20260528082314-Gate-skill-store-startup-sync/audit.lisp"]
       :acceptance ["cargo check -p missiond-daemon"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "git diff --check"])))
