(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260525113426-Add-localhost-only-Jarvis-slot-e"
  :intent "wo-20260525113426-Add-localhost-only-Jarvis-slot-e"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260525113426-Add-localhost-only-Jarvis-slot-e-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/memory-knowledge-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     ".missiond/workflows/missiond-macmini-self-update.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/check-v3-macmini-self-update-lane.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-macmini-self-update-lane.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
