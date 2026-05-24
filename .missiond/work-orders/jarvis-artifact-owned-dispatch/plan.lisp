(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "jarvis-artifact-owned-dispatch"
  :intent "jarvis-artifact-owned-dispatch"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "jarvis-artifact-owned-dispatch-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/jarvis-artifact-owned-dispatch/plan.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     ".missiond/workflows/work-order-lifecycle.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-workflow-isomorphism.mjs --engine=ocaml --json"
                    "node scripts/check-v3-workstation-pool-isomorphism.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "cargo test -p missiond-core jarvis -- --nocapture"
                    "cargo test -p missiond-daemon append_board_task_id_suffix -- --nocapture"
                    "cargo test -p missiond-daemon output_contract_close_blocker -- --nocapture"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
