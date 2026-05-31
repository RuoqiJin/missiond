(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531084422-Implement-provider-box-interacti"
  :intent "wo-20260531084422-Implement-provider-box-interacti"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531084422-Implement-provider-box-interacti-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/evidence/workstation-pool.lisp"
                     ".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/universe/service-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/lib.rs"
                     "crates/missiond-core/src/ws/mod.rs"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/provider_box"
                     "crates/missiond-daemon/src/state.rs"
                     "crates/missiond-pty/src/pty_recognition.rs"
                     "scripts/check-v3-code-isomorphism-complete.mjs"
                     "scripts/check-v3-interactive-provider-box.mjs"]
       :acceptance ["cargo check -p missiond-daemon"
                    "cargo test -p missiond-daemon provider_box -- --nocapture"
                    "cargo test -p missiond-pty agy -- --nocapture"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"])))
