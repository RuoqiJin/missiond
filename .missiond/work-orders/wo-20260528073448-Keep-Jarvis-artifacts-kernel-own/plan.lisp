(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528073448-Keep-Jarvis-artifacts-kernel-own"
  :intent "wo-20260528073448-Keep-Jarvis-artifacts-kernel-own"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528073448-Keep-Jarvis-artifacts-kernel-own-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/engine/control_plane_kernel.rs"
                     ".missiond/work-orders/wo-20260528073448-Keep-Jarvis-artifacts-kernel-own"]
       :acceptance ["rustfmt --edition 2021 --check crates/missiond-daemon/src/main.rs crates/missiond-daemon/src/engine/control_plane_kernel.rs"
                    "cargo check -p missiond-daemon"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "git diff --check"])))
