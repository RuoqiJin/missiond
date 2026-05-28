(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528082648-Upsert-EventBridge-task-contract"
  :intent "wo-20260528082648-Upsert-EventBridge-task-contract"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528082648-Upsert-EventBridge-task-contract-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/bus/v2_subscribers.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528082648-Upsert-EventBridge-task-contract/intent.lisp"
                     ".missiond/work-orders/wo-20260528082648-Upsert-EventBridge-task-contract/plan.lisp"
                     ".missiond/work-orders/wo-20260528082648-Upsert-EventBridge-task-contract/audit.lisp"]
       :acceptance ["cargo check -p missiond-daemon"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "git diff --check"])))
