(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528095500-Fail-closed-shared-memory-adapter-fallback"
  :intent "wo-20260528095500-Fail-closed-shared-memory-adapter-fallback"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528095500-Fail-closed-shared-memory-adapter-fallback-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     "scripts/check-v3-shared-memory-isomorphism.mjs"]
       :acceptance ["cargo test -p missiond-daemon handlers::knowledge::shared_memory::tests -- --nocapture"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-shared-memory-isomorphism.mjs --json"
                    "git diff --check"])))
