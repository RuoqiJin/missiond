;; Wave 51 context-pack: already-integrated single-shard plan.

(context-pack wave51-context-pack
  :schema "missiond.context-pack.v1"
  :wave wave51
  :purpose "Two-stage context-pack test for a code-worker shard: Autopilot must launch pty.send concurrently across different slots in one dispatch tick."
  :write-model append-only
  :sequence 3

  (observation :id wave51-obs-serial-send-loop
    :agent codex-parent
    :seq 1
    :at "2026-04-29T09:25:00Z"
    :summary "Live delegated runs showed that Autopilot can claim and send one BoardTask, then block the dispatch loop inside state.pty.send(...).await before it reaches another ready task assigned to a different slot. The existing per-slot dispatch guard is correct for same-slot exclusion, but the for-task loop still serializes different-slot sends. This is V3 workstation-config drift: holding a per-slot guard across send must not starve callers or tasks targeting other slots."
    :files ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
            "crates/missiond-daemon/src/slot_dispatch.rs"
            ".missiond/v3/missiond-blueprint.lisp"
            "scripts/check-v3-workstation-config-isomorphism.mjs"])

  (shard-proposal :id wave51-shard-concurrent-slot-send
    :agent codex-parent
    :seq 2
    :at "2026-04-29T09:25:05Z"
    :summary "Implementation shard: split Autopilot BoardTask dispatch into a preparation/claim phase and a send/completion phase so every ready task for a different slot can reach the send phase in the same dispatch tick. Preserve the per-slot RAII guard across each individual state.pty.send call, but do not await one slot's send before starting another slot's send. Prefer a small helper or future collection that keeps existing prompt assembly, claim, lease, prompt snapshot, event emission, close-owner, failure, KB feedback, and deploy-review behavior intact. Update V3 blueprint + workstation-config checker so the invariant is pinned."
    :shard concurrent-slot-send
    :owner claudecode
    :write-scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                  ".missiond/v3/missiond-blueprint.lisp"
                  "scripts/check-v3-workstation-config-isomorphism.mjs"]
    :must-not-touch ["packages/**"
                     ".missiond/v1/**"
                     ".missiond/v2/**"
                     ".missiond/tasks/wave48/**"
                     ".missiond/tasks/wave49/**"
                     ".missiond/tasks/wave50/**"
                     ".missiond/tasks/wave51/manifest.lisp"
                     ".missiond/tasks/wave51/wave51-*.lisp"
                     ".missiond/tasks/wave51/context-atlas.lisp"
                     ".missiond/tasks/wave51/pattern-cards.lisp"
                     ".missiond/tasks/wave51/context-pack.lisp"
                     ".missiond/claudecode/**"
                     "scripts/check-context-pack.mjs"
                     "scripts/context-pack-append.mjs"
                     "scripts/context-pack-compile-shards.mjs"
                     "scripts/check-v3-context-pack-isomorphism.mjs"]
    :acceptance ["node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture"
                 "node scripts/check-v3-workstation-config-isomorphism.mjs"
                 "node scripts/check-v3-code-isomorphism-complete.mjs"
                 "cargo check -p missiond-daemon"
                 "cargo test -p missiond-daemon engine::intent_engine::autopilot::tests -- --nocapture"
                 "git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workstation-config-isomorphism.mjs"])

  (integration-plan :id wave51-integration-plan-001
    :agent codex-integrator
    :seq 3
    :at "2026-04-29T09:25:10Z"
    :summary "Accepted one implementation shard. This wave deliberately uses mapped dispatch-groups so the worker brief consumes context-pack-compile-shards output without narrative parsing."
    :accepted-shards [concurrent-slot-send]
    :dispatch-groups [(group :id A :shards [concurrent-slot-send])])
)
