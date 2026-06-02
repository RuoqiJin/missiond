(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602085705-Implement-deployment-closure-pol"
  :intent "wo-20260602085705-Implement-deployment-closure-pol"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602085705-Implement-deployment-closure-pol-shard-default"
       :read_scope ["."]
       :write_scope ["scripts/compile-v3-runtime.mjs"
                     "scripts/deploy-daemon.sh"
                     "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
                     "crates/missiond-daemon/src/context/v3_blueprint_runtime/compiled_snapshot.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/project.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"]
       :acceptance ["node scripts/compile-v3-runtime.mjs --check --json"
                    "cargo check -p missiond-daemon"])))
