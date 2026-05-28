(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528035404-Guard-MissionD-searches-from-col"
  :intent "wo-20260528035404-Guard-MissionD-searches-from-col"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528035404-Guard-MissionD-searches-from-col-shard-default"
       :read_scope ["."]
       :write_scope [".ignore"
                     ".missiond/.ignore"
                     ".missiond/research/.ignore"
                     ".missiond/tasks/.ignore"
                     ".missiond/v3/.ignore"
                     ".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/implementation/ops-surfaces.lisp"
                     ".missiond/v3/shards/pillar-flow-map.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-runtime-path-hygiene.mjs"
                     "scripts/check-v3-source-hygiene-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260528035404-Guard-MissionD-searches-from-col/**"]
       :acceptance ["node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "node scripts/check-v3-source-hygiene-isomorphism.mjs --json"])))
