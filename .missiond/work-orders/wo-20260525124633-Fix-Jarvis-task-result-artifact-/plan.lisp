(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260525124633-Fix-Jarvis-task-result-artifact-"
  :intent "wo-20260525124633-Fix-Jarvis-task-result-artifact-"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260525124633-Fix-Jarvis-task-result-artifact--shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/engine/shared_memory.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/check-v3-runtime-domain-projections.mjs --json"
                    "cargo check -p missiond-daemon"])))
