(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603060836-Fix-Jarvis-public-SSE-disconnect"
  :intent "wo-20260603060836-Fix-Jarvis-public-SSE-disconnect"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603060836-Fix-Jarvis-public-SSE-disconnect-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"]
       :acceptance ["cargo fmt --check"
                    "cargo test -p missiond-core jarvis_sse_disconnect_errors_are_non_terminal -- --nocapture"
                    "cargo test -p missiond-core public_jarvis_prefix_normalizes_to_daemon_routes -- --nocapture"])))
