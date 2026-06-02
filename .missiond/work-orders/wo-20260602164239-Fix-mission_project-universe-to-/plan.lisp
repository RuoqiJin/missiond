(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602164239-Fix-mission_project-universe-to-"
  :intent "wo-20260602164239-Fix-mission_project-universe-to-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602164239-Fix-mission_project-universe-to--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/knowledge/project/universe.rs"]
       :acceptance ["rustfmt --edition 2021 --check crates/missiond-daemon/src/handlers/knowledge/project/universe.rs"
                    "cargo test -p missiond-daemon compiled_service_output_preserves_runtime_support_catalog"
                    "cargo check -p missiond-daemon"])))
