(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260606055042-Fix-context-pack-project-resolut"
  :intent "wo-20260606055042-Fix-context-pack-project-resolut"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260606055042-Fix-context-pack-project-resolut-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/pillar-flow-map.lisp"
                     ".missiond/v3/shards/v2-convergence-map.lisp"
                     "crates/missiond-core/src/v3_contracts.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-codex-boot-context-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/mission-context-pack.mjs"
                     "scripts/mission-mcp-call.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "node scripts/check-v3-codex-boot-context-isomorphism.mjs --json"])))
