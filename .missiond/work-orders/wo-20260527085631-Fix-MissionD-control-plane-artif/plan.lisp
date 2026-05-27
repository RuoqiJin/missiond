(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527085631-Fix-MissionD-control-plane-artif"
  :intent "wo-20260527085631-Fix-MissionD-control-plane-artif"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527085631-Fix-MissionD-control-plane-artif-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v2/**"
                     ".missiond/v3/**"
                     "crates/missiond-core/**"
                     "crates/missiond-daemon/**"
                     "crates/missiond-mcp/**"
                     "scripts/**"
                     "tools/missiond_lispc/**"]
       :acceptance ["bash scripts/rustfmt-missiond.sh --check"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo test -p missiond-daemon output_contract -- --nocapture"
                    "cargo test -p missiond-daemon task_result_artifact -- --nocapture"
                    "cargo test -p missiond-core board -- --nocapture"])))
