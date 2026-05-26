(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526122316-Add-conversation-session-context"
  :intent "wo-20260526122316-Add-conversation-session-context"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526122316-Add-conversation-session-context-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/memory-knowledge-runtime.lisp"
                     "crates/missiond-core/src/db/pg/conversation.rs"
                     "crates/missiond-core/src/db/traits.rs"
                     "crates/missiond-core/src/types/conversation.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_capsule.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/mod.rs"
                     "crates/missiond-daemon/src/infra/message_handler.rs"
                     "crates/missiond-daemon/src/slot_orchestrator/mod.rs"
                     "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
                     "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
                     "scripts/check-v3-conversation-session-management.mjs"
                     "scripts/check-v3-runtime-path-hygiene.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["node scripts/check-v3-conversation-session-management.mjs --json"
                    "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "cargo check -p missiond-core -p missiond-daemon"
                    "scripts/cargo-fmt-touched.sh --check"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
