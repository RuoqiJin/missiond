(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526171259-Fix-task-result-artifact-authority"
  :intent "wo-20260526171259-Fix-task-result-artifact-authority"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526171259-Fix-task-result-artifact-authority-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260526171259-Fix-task-result-artifact-authority/**"
                     ".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/universe/behavior-closure.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"]
       :acceptance ["cargo test -p missiond-daemon autopilot --lib"
                    "cargo test -p missiond-core jarvis --lib"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "scripts/cargo-fmt-touched.sh --check"
                    "git diff --check"])))
