(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-context-gather-noise-fix"
  :intent "wo-context-gather-noise-fix"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-context-gather-noise-fix-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/db"
                     "crates/missiond-daemon/src/handlers/comm/conversation"
                     "crates/missiond-daemon/src/handlers/knowledge"
                     "crates/missiond-daemon/src/handlers/sysinfra"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-mcp/src/tools/knowledge"
                     "crates/missiond-mcp/src/tools/sysinfra"
                     "scripts/generated"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     "scripts/check-v3-source-hygiene-isomorphism.mjs"]
       :acceptance ["node scripts/project-v3-contracts.mjs --verify"
                    "node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                    "node scripts/check-v3-source-hygiene-isomorphism.mjs --json"
                    "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "node scripts/check-v3-shared-memory-isomorphism.mjs --json"
                    "cargo test -p missiond-daemon --bin missiond context_gather::tests"
                    "cargo test -p missiond-daemon --bin missiond sysinfra::infra::tests"
                    "cargo test -p missiond-daemon --bin missiond project::registry::tests"])))
