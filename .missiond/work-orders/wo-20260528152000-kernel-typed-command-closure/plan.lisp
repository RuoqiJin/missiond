(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528152000-kernel-typed-command-closure"
  :intent "wo-20260528152000-kernel-typed-command-closure"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528152000-kernel-typed-command-closure-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/control_plane_kernel.rs"
                     "crates/missiond-daemon/src/engine/shared_memory.rs"
                     "crates/missiond-daemon/src/engine/task_completion_evidence.rs"
                     "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528152000-kernel-typed-command-closure/plan.lisp"
                     ".missiond/work-orders/wo-20260528152000-kernel-typed-command-closure/intent.lisp"
                     ".missiond/work-orders/wo-20260528152000-kernel-typed-command-closure/audit.lisp"]
       :acceptance ["node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
