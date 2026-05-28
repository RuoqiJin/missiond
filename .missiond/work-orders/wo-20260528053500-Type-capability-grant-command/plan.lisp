(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528053500-Type-capability-grant-command"
  :intent "wo-20260528053500-Type-capability-grant-command"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528053500-Type-capability-grant-command-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/control_plane_kernel.rs"
                     "crates/missiond-daemon/src/engine/shared_memory.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     ".missiond/v3/shards/universe/behavior-closure.lisp"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
