(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602083048-Compact-MissionD-context-gather-"
  :intent "wo-20260602083048-Compact-MissionD-context-gather-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260602083048-Compact-MissionD-context-gather--shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/memory-knowledge-runtime.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/universe/service-runtime.lisp"
                     "crates/missiond-core/src/db/pg/knowledge.rs"
                     "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
                     "crates/missiond-daemon/src/context/v3_blueprint_runtime/runtime_config_payload.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"
                     "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
                     "crates/missiond-daemon/src/provider_box/claude_code_driver.rs"
                     "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
                     "scripts/analyze-codex-deployment-day.mjs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/check-v3-memory-kb-isomorphism.mjs"
                     "scripts/check-v3-project-registry-isomorphism.mjs"
                     "scripts/check-v3-skill-runtime-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/lib/v3_runtime_domains.mjs"
                     "tools/missiond_lispc/bin/emit_json.ml"]
       :acceptance ["cargo test -p missiond-daemon --bin missiond context_gather::tests"
                    "cargo test -p missiond-daemon --bin missiond sysinfra::infra::tests"
                    "node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                    "node scripts/check-v3-source-hygiene-isomorphism.mjs --json"
                    "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "node scripts/check-v3-shared-memory-isomorphism.mjs --json"
                    "cargo check -p missiond-daemon -p missiond-mcp"])))
