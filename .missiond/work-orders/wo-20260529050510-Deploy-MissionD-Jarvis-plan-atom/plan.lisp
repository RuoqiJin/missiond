(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260529050510-Deploy-MissionD-Jarvis-plan-atom"
  :intent "wo-20260529050510-Deploy-MissionD-Jarvis-plan-atom"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260529050510-Deploy-MissionD-Jarvis-plan-atom-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3"
                     ".missiond/workflows"
                     "Cargo.toml"
                     "crates/missiond-core/src"
                     "crates/missiond-daemon/src"
                     "scripts"
                     "tools/missiond_lispc"
                     "docs/guides"]
       :acceptance ["node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-work-order-lifecycle-isomorphism.mjs --json"
                    "node scripts/check-v3-board-isomorphism.mjs --json"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "scripts/cargo-fmt-touched.sh --check"
                    "git diff --check"])))
