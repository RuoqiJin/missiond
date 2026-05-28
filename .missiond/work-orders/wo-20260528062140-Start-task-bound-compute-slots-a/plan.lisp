(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528062140-Start-task-bound-compute-slots-a"
  :intent "wo-20260528062140-Start-task-bound-compute-slots-a"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528062140-Start-task-bound-compute-slots-a-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
                     ".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/control-plane-runtime.lisp"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260528062140-Start-task-bound-compute-slots-a"]
       :acceptance ["bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-daemon"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
