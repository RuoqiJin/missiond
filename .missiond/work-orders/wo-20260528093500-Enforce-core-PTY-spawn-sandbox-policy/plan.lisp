(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528093500-Enforce-core-PTY-spawn-sandbox-policy"
  :intent "wo-20260528093500-Enforce-core-PTY-spawn-sandbox-policy"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528093500-Enforce-core-PTY-spawn-sandbox-policy-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-pty/src/manager.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"]
       :acceptance ["cargo test -p missiond-pty manager::tests -- --nocapture"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "git diff --check"])))
