(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-202605270002-jarvis-follow-short-poll"
  :intent "wo-202605270002-jarvis-follow-short-poll"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-202605270002-jarvis-follow-short-poll-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-202605270002-jarvis-follow-short-poll/intent.lisp"
                     ".missiond/work-orders/wo-202605270002-jarvis-follow-short-poll/plan.lisp"
                     ".missiond/work-orders/wo-202605270002-jarvis-follow-short-poll/audit.lisp"]
       :acceptance ["cargo check -p missiond-core"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
