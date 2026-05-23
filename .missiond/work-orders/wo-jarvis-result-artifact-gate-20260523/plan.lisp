(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-jarvis-result-artifact-gate-20260523"
  :intent "wo-jarvis-result-artifact-gate-20260523"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-jarvis-result-artifact-gate-20260523-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo check -p missiond-core -p missiond-daemon"
                    "bash scripts/rustfmt-missiond.sh --check"
                    "git diff --check"])))
