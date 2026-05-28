(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528072115-Keep-lease-adapter-parser-tests-"
  :intent "wo-20260528072115-Keep-lease-adapter-parser-tests-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528072115-Keep-lease-adapter-parser-tests--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/shared_memory.rs"
                     ".missiond/work-orders/wo-20260528072115-Keep-lease-adapter-parser-tests-/**"]
       :acceptance ["rustfmt --edition 2021 --check crates/missiond-daemon/src/engine/shared_memory.rs"
                    "cargo test -p missiond-daemon engine::shared_memory::tests::release_lease_request_from_args_preserves_authority_and_scope engine::shared_memory::tests::heartbeat_lease_request_from_args_requires_explicit_operator_confirm_for_bypass"
                    "git diff --check"])))
