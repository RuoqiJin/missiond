(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "bf973163-c057-4e07-8924-1ca0eb315a3d"
  :intent "bf973163-c057-4e07-8924-1ca0eb315a3d"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "bf973163-c057-4e07-8924-1ca0eb315a3d-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/cc_tasks/types.rs"
                     "crates/missiond-core/src/gemini_cli/parser.rs"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/events_sync.rs"
                     "crates/missiond-daemon/src/infra/message_handler.rs"
                     "crates/missiond-daemon/src/workers/local/reconcile_worker.rs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/smoke-jarvis-intent-plan-dispatch.mjs"]
       :acceptance ["scripts/cargo-fmt-touched.sh --check"
                    "cargo test -p missiond-daemon -- autopilot --nocapture"
                    "cargo test -p missiond-core -- ws::server --nocapture"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-runtime-domain-projections.mjs --json"
                    "git diff --check"])))
