(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528082000-Board-done-settle-authority"
  :intent "wo-20260528082000-Board-done-settle-authority"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528082000-Board-done-settle-authority-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
