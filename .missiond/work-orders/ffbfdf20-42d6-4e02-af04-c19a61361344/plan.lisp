(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "ffbfdf20-42d6-4e02-af04-c19a61361344"
  :intent "ffbfdf20-42d6-4e02-af04-c19a61361344"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "ffbfdf20-42d6-4e02-af04-c19a61361344-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp" "crates/missiond-pty/src/pty_recognition.rs" "crates/missiond-pty/src/session.rs" "scripts/check-v3-agent-cli-regression.mjs" "scripts/check-v3-pty-recognition-isomorphism.mjs"]
       :acceptance ["cargo test -p missiond-pty agy_ -- --nocapture" "cargo check -p missiond-pty" "node scripts/check-v3-agent-cli-regression.mjs --json" "node scripts/check-v3-pty-recognition-isomorphism.mjs --json" "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
