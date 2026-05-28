(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528051500-Route-mission-execution-lease-updates"
  :intent "wo-20260528051500-Route-mission-execution-lease-updates"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528051500-Route-mission-execution-lease-updates-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_release.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_heartbeat.rs"
                     "crates/missiond-daemon/src/engine/control_plane_kernel.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs"
                     "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/v3/shards/**"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
