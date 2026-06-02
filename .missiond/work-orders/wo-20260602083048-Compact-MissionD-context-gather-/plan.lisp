(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602083048-Compact-MissionD-context-gather-"
  :intent "wo-20260602083048-Compact-MissionD-context-gather-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602083048-Compact-MissionD-context-gather--shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"]
       :acceptance ["cargo test -p missiond-daemon --bin missiond context_gather::tests"
                    "cargo test -p missiond-daemon --bin missiond sysinfra::infra::tests"
                    "node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                    "node scripts/check-v3-source-hygiene-isomorphism.mjs --json"
                    "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "node scripts/check-v3-shared-memory-isomorphism.mjs --json"
                    "cargo check -p missiond-daemon -p missiond-mcp"])))
