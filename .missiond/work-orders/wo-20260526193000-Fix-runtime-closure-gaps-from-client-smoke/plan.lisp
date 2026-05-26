(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526193000-Fix-runtime-closure-gaps-from-client-smoke"
  :intent "wo-20260526193000-Fix-runtime-closure-gaps-from-client-smoke"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526193000-Fix-runtime-closure-gaps-from-client-smoke-shard-default"
       :read_scope ["."]
       :write_scope [
         ".missiond/work-orders/wo-20260526193000-Fix-runtime-closure-gaps-from-client-smoke/**"
         ".missiond/v3/shards/request-runtime.lisp"
         ".missiond/v3/shards/workstation-runtime.lisp"
         ".missiond/v3/shards/memory-knowledge-runtime.lisp"
         ".missiond/v3/shards/control-plane-runtime.lisp"
         "crates/missiond-core/src/cc_tasks/watcher.rs"
         "crates/missiond-core/src/db/pg/conversation.rs"
         "crates/missiond-core/src/db/traits.rs"
         "crates/missiond-core/src/event/pipeline/step7_fanout/mod.rs"
         "crates/missiond-core/src/event/subscription/mod.rs"
         "crates/missiond-core/tests/event_chaos.rs"
         "crates/missiond-daemon/src/events_sync.rs"
         "crates/missiond-daemon/src/workers/local/conversation_logger.rs"
         "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
         "scripts/deploy-daemon.sh"
         "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
         "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
         "scripts/generated/v3_contracts.d.ts"
         "scripts/generated/v3_contracts.mjs"
         "scripts/generated/v3_runtime_defaults.mjs"
       ]
       :acceptance [
         "cargo test -p missiond-daemon timeline_analyst"
         "cargo test -p missiond-daemon conversation_events"
         "cargo test -p missiond-core event_chaos::chaos_5_slow_subscriber_lag"
         "node scripts/check-v3-code-isomorphism-complete.mjs --json"
         "node scripts/check-v3-final-convergence.mjs --json --static-only"
         "scripts/cargo-fmt-touched.sh --check"
         "git diff --check"
       ])))
