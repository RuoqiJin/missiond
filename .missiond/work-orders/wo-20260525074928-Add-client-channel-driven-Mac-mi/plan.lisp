(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260525074928-Add-client-channel-driven-Mac-mi"
  :intent "wo-20260525074928-Add-client-channel-driven-Mac-mi"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260525074928-Add-client-channel-driven-Mac-mi-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/workflows/missiond-macmini-self-update.lisp"
                     ".missiond/v3/shards/universe/behavior-closure.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-code-isomorphism-complete.mjs"
                     "scripts/check-v3-workflow-isomorphism.mjs"
                     "scripts/check-v3-macmini-self-update-lane.mjs"
                     "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-macmini-self-update-lane.mjs --json"
                    "node scripts/check-v3-workflow-isomorphism.mjs --engine=ocaml --json"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "node scripts/smoke-jarvis-chain.mjs --json --allow-busy"
                    "node scripts/smoke-jarvis-interaction.mjs --json"
                    "node scripts/smoke-jarvis-intent-plan-dispatch.mjs --json"])))
