(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527051807-Reject-echoed-worker-prompt-cont"
  :intent "wo-20260527051807-Reject-echoed-worker-prompt-cont"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527051807-Reject-echoed-worker-prompt-cont-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon extract_worker_final_summary -- --nocapture"
                   "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                   "node scripts/check-v3-final-convergence.mjs --json --static-only"
                   "git diff --check"])))
