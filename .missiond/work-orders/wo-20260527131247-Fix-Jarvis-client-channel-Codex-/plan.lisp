(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527131247-Fix-Jarvis-client-channel-Codex-"
  :intent "wo-20260527131247-Fix-Jarvis-client-channel-Codex-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527131247-Fix-Jarvis-client-channel-Codex--shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/control-plane-runtime.lisp"
                     ".missiond/v3/shards/universe/behavior-closure.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot_workflow.rs"
                     "crates/missiond-daemon/src/feature_gates.rs"
                     "crates/missiond-daemon/src/handlers/mod.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "scripts/check-v3-agent-cli-regression.mjs"
                     "scripts/check-v3-autopilot-runtime-isomorphism.mjs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"
                     "scripts/deploy-daemon.sh"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon intent_engine::autopilot --quiet"
                    "cargo test -p missiond-daemon feature_gates --quiet"
                    "cargo check -p missiond-daemon"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-autopilot-runtime-isomorphism.mjs --json"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
