(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526132933-Fix-Jarvis-grounded-context-visi"
  :intent "wo-20260526132933-Fix-Jarvis-grounded-context-visi"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526132933-Fix-Jarvis-grounded-context-visi-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-pty/src/pty_recognition.rs"
                     "scripts/check-v3-agent-cli-regression.mjs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/check-v3-runtime-path-hygiene.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-pty agy_file_access_prompt_is_blocked_confirmation"
                   "cargo test -p missiond-core jarvis_dispatch_context_pack_parent_enters_read_scope"
                   "node scripts/check-v3-agent-cli-regression.mjs --json"
                   "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                   "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                   "cargo check -p missiond-daemon"
                   "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
