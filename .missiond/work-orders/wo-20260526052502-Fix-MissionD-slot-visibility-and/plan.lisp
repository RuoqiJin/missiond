(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526052502-Fix-MissionD-slot-visibility-and"
  :intent "wo-20260526052502-Fix-MissionD-slot-visibility-and"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526052502-Fix-MissionD-slot-visibility-and-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/implementation/runtime-surfaces.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "scripts/check-v3-agent-cli-regression.mjs"
                     "scripts/check-v3-workstation-pool-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-core -p missiond-daemon"
                    "node scripts/check-v3-workstation-pool-isomorphism.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/check-v3-runtime-domain-projections.mjs --json"
                    "git diff --check"])))
