(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528153500-xjpcode-attempt-binding"
  :intent "wo-20260528153500-xjpcode-attempt-binding"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528153500-xjpcode-attempt-binding-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528153500-xjpcode-attempt-binding/plan.lisp"
                     ".missiond/work-orders/wo-20260528153500-xjpcode-attempt-binding/intent.lisp"
                     ".missiond/work-orders/wo-20260528153500-xjpcode-attempt-binding/audit.lisp"]
       :acceptance ["node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
