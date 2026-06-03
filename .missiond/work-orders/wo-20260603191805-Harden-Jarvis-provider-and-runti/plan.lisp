(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603191805-Harden-Jarvis-provider-and-runti"
  :intent "wo-20260603191805-Harden-Jarvis-provider-and-runti"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603191805-Harden-Jarvis-provider-and-runti-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/frontend"
                     ".missiond/v3"
                     ".missiond/workflows"
                     "crates/missiond-core"
                     "crates/missiond-daemon"
                     "crates/missiond-mcp"
                     "crates/missiond-pty"
                     "docs/guides"
                     "packages/board"
                     "scripts"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
