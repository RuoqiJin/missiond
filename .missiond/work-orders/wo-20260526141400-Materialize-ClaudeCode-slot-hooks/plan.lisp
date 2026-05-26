(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526141400-Materialize-ClaudeCode-slot-hooks"
  :intent "wo-20260526141400-Materialize-ClaudeCode-slot-hooks"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526141400-Materialize-ClaudeCode-slot-hooks-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     ".missiond/work-orders/wo-20260526141400-Materialize-ClaudeCode-slot-hooks/"
                     "crates/missiond-daemon/src/context/slot_env.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "scripts/check-v3-workstation-config-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon slot_env"
                    "node scripts/check-v3-workstation-config-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
