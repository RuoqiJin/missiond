(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528142000-kernel-task-contract-entry"
  :intent "wo-20260528142000-kernel-task-contract-entry"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528142000-kernel-task-contract-entry-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/control_plane_kernel.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
                     "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
                     "crates/missiond-daemon/src/handlers/compute/pty.rs"
                     "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528142000-kernel-task-contract-entry/intent.lisp"
                     ".missiond/work-orders/wo-20260528142000-kernel-task-contract-entry/plan.lisp"
                     ".missiond/work-orders/wo-20260528142000-kernel-task-contract-entry/audit.lisp"]
       :acceptance ["node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-daemon"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
