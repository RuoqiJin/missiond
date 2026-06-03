(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-fullos-background-db-grace"
  :intent "wo-fullos-background-db-grace"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-fullos-background-db-grace-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/control-plane-runtime.lisp"
                     "crates/missiond-core/src/db/pg/mod.rs"
                     "crates/missiond-daemon/src/workers/local/mod.rs"
                     "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
                     "crates/missiond-daemon/src/workers/local/conversation_organizer.rs"
                     "crates/missiond-daemon/src/workers/local/message_labeler.rs"
                     "crates/missiond-daemon/src/workers/local/tagger_chunker.rs"
                     "scripts/deploy-daemon.sh"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo fmt --all --check"
                    "cargo test -p missiond-daemon --bin missiond conversation::query::tests"
                    "cargo test -p missiond-core --lib db::pg"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-conversation-ingestion-isomorphism.mjs --json"
                    "node scripts/project-v3-contracts.mjs --check --json"])))
