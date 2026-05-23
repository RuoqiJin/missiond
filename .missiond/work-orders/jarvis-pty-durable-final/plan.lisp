(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "jarvis-pty-durable-final"
  :intent "jarvis-pty-durable-final"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "jarvis-pty-durable-final-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/handlers/compute/pty.rs"
                     "crates/missiond-pty/src/pty_recognition.rs"]
       :acceptance ["cargo check -p missiond-core"
                   "cargo check -p missiond-daemon"
                   "cargo test -p missiond-pty claude_code_idle_prompt_overrides_stale_spinner_text"
                   "node scripts/check-v3-final-convergence.mjs --json --static-only"
                   "git diff --check"])))
