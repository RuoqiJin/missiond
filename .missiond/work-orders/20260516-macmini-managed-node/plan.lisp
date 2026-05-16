(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260516-macmini-managed-node"
  :intent "20260516-macmini-managed-node"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260516-macmini-managed-node-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/missiond-blueprint.lisp"
                     "crates/missiond-core/src/cc_tasks/watcher.rs"
                     "crates/missiond-core/src/db/pg/skill.rs"
                     "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
                     "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/project/universe.rs"
                     "crates/missiond-daemon/src/helpers.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/workers/local/experience_harvester.rs"
                     "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
                     "scripts/check-v3-skill-runtime-isomorphism.mjs"
                     "scripts/check-v3-workstation-config-isomorphism.mjs"]
       :acceptance ["scripts/cargo-fmt-touched.sh --check"
                    "node scripts/check-v3-skill-runtime-isomorphism.mjs --json"
                    "node scripts/check-v3-workstation-config-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo check -p missiond-core -p missiond-daemon -p missiond-mcp"
                    "cargo build -p missiond-daemon -p missiond-mcp --release"
                    "git diff --check"])))
