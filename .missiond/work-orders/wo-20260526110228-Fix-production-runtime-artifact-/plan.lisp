(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526110228-Fix-production-runtime-artifact-"
  :intent "wo-20260526110228-Fix-production-runtime-artifact-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526110228-Fix-production-runtime-artifact--shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/control-plane-runtime.lisp"
                     ".missiond/v3/shards/implementation/runtime-surfaces.lisp"
                     ".missiond/v3/shards/pillar-flow-map.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/master_control.rs"
                     "crates/missiond-daemon/src/organism/autopilot_organ.rs"
                     "scripts/check-v3-runtime-path-hygiene.mjs"
                     "scripts/deploy-daemon.sh"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo check -p missiond-daemon"])))
