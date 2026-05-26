(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526160000-Fix-readonly-dispatch-provider-drift"
  :intent "wo-20260526160000-Fix-readonly-dispatch-provider-drift"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526160000-Fix-readonly-dispatch-provider-drift-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260526160000-Fix-readonly-dispatch-provider-drift/**"
                     ".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-core/src/types/gen_types.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-agent-cli-regression.mjs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-core jarvis_dispatch"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
