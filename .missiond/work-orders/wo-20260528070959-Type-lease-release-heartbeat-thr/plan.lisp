(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528070959-Type-lease-release-heartbeat-thr"
  :intent "wo-20260528070959-Type-lease-release-heartbeat-thr"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528070959-Type-lease-release-heartbeat-thr-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/control_plane_kernel.rs"
                     "crates/missiond-daemon/src/engine/shared_memory.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     ".missiond/work-orders/wo-20260528070959-Type-lease-release-heartbeat-thr/**"]
       :acceptance ["bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-daemon"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
