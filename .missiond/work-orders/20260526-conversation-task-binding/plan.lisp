(plan conversation-task-binding
  :schema "missiond.work-order.plan.v1"
  :intent conversation-task-binding
  :accepted_shards [conversation-binding-guard completion-final-claim-window ssot-checker-test]
  (shard conversation-binding-guard
    :write_scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"]
    :steps ["Guard dispatch-time conversation rebind so it only applies to active, not-ended conversations."
            "Do not overwrite task_id on completed historical conversations still present in slot_sessions."])
  (shard completion-final-claim-window
    :write_scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"]
    :steps ["Filter durable provider completion candidates whose ended_at is before the BoardTask claimed_at."
            "Keep current active slot session eligible after session capture."])
  (shard ssot-checker-test
    :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                  ".missiond/v3/shards/universe/behavior-closure.lisp"
                  "scripts/check-v3-workstation-pool-isomorphism.mjs"
                  "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                  "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                  "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                  "scripts/generated/v3_contracts.d.ts"
                  "scripts/generated/v3_contracts.mjs"
                  "scripts/generated/v3_runtime_defaults.mjs"]
    :acceptance ["cargo test -p missiond-daemon conversation_ended_before_claim_rejects_stale_final"
                 "cargo test -p missiond-daemon dispatch_rebind_skips_completed_conversation"
                 "node scripts/check-v3-workstation-pool-isomorphism.mjs --json"
                 "scripts/cargo-fmt-touched.sh --check"
                 "git diff --check"]))
