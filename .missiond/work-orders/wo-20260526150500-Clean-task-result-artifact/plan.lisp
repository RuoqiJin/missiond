(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526150500-Clean-task-result-artifact"
  :intent "wo-20260526150500-Clean-task-result-artifact"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526150500-Clean-task-result-artifact-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260526150500-Clean-task-result-artifact/**"
                     ".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/universe/behavior-closure.lisp"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/engine/shared_memory.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon extract_worker_final_summary"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
