(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260529062555-Fix-Jarvis-intent-plan-authoring"
  :intent "wo-20260529062555-Fix-Jarvis-intent-plan-authoring"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260529062555-Fix-Jarvis-intent-plan-authoring-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"]
       :acceptance ["cargo check -p missiond-core"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "git diff --check"])))
