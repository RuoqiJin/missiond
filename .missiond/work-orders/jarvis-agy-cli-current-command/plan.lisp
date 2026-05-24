(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "jarvis-agy-cli-current-command"
  :intent "jarvis-agy-cli-current-command"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "jarvis-agy-cli-current-command-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/evidence/workstation-pool.lisp"
                     "crates/missiond-pty/src/session.rs"
                     "scripts/check-v3-agent-cli-regression.mjs"
                     "scripts/check-v3-workstation-pool-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-agent-cli-regression.mjs --json"
                   "node scripts/check-v3-workstation-pool-isomorphism.mjs --json"
                   "cargo test -p missiond-pty agy -- --nocapture"
                   "bash scripts/rustfmt-missiond.sh --check"
                   "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
