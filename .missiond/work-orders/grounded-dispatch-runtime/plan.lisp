(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "grounded-dispatch-runtime"
  :intent "grounded-dispatch-runtime"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "grounded-dispatch-runtime-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/workflows/intent-intake-grounding.lisp"
                     ".missiond/workflows/work-order-lifecycle.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/engine/shared_memory.rs"
                     "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                   "node scripts/check-v3-workflow-isomorphism.mjs --engine=ocaml --json"
                   "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                   "node scripts/check-v3-final-convergence.mjs --json --static-only"
                   "cargo check -p missiond-daemon"
                   "cargo test -p missiond-daemon task_delegate"
                   "git diff --check"])))
