(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528185558-kernel-capability-audit-command"
  :intent "wo-20260528185558-kernel-capability-audit-command"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528185558-kernel-capability-audit-command-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/control_plane_kernel.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
                     "crates/missiond-daemon/src/handlers/compute/pty.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528185558-kernel-capability-audit-command/intent.lisp"
                     ".missiond/work-orders/wo-20260528185558-kernel-capability-audit-command/plan.lisp"
                     ".missiond/work-orders/wo-20260528185558-kernel-capability-audit-command/audit.lisp"]
       :acceptance ["node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
