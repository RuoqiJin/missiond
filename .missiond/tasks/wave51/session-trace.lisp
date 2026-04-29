(session-trace wave51
  :schema "missiond.session-trace.v1"
  :wave wave51
  :created-at "2026-04-29T09:25:00Z"
  :sequence 0


  (trace-event
    :id wave51-trace-bootstrap-start-001
    :seq 1
    :at "2026-04-29T08:35:16Z"
    :task wave51-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave51-trace-bootstrap-read-001
    :seq 2
    :at "2026-04-29T08:35:16Z"
    :task wave51-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave51-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave51-01-trace-preamble-read-001
    :seq 3
    :at "2026-04-29T10:05:00Z"
    :task wave51-01-autopilot-concurrent-slot-dispatch-v0
    :backend claudecode
    :kind read
    :files [".missiond/claudecode/wave51-shared-preamble.md"
            ".missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp"
            ".missiond/tasks/wave51/context-atlas.lisp"
            ".missiond/tasks/wave51/pattern-cards.lisp"
            ".missiond/tasks/wave51/context-pack.lisp"]
    :summary "Loaded shared preamble + task contract + atlas + pattern cards + context-pack integration plan before broad scans, per the audit expectation pinned by the wave51 bootstrap.")

  (trace-event
    :id wave51-01-trace-observation-001
    :seq 4
    :at "2026-04-29T10:25:00Z"
    :task wave51-01-autopilot-concurrent-slot-dispatch-v0
    :backend claudecode
    :kind observation
    :files ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
            "crates/missiond-daemon/src/slot_dispatch.rs"]
    :summary "Confirmed legacy dispatch_board_tasks awaited state.pty.send inline inside the for-task loop, serializing different-slot sends. The per-slot SlotAcquireGuard borrows &SlotDispatchGuard which prevents moving the guard into a tokio::task::JoinSet send-task; introduced OwnedSlotDispatchGuard wrapping Arc<SlotDispatchGuard> + slot_id with Drop->release so the guard travels into a 'static spawn while keeping per-slot exclusion across the post-send tail.")

  (trace-event
    :id wave51-01-trace-acceptance-001
    :seq 5
    :at "2026-04-29T10:35:00Z"
    :task wave51-01-autopilot-concurrent-slot-dispatch-v0
    :backend claudecode
    :kind acceptance
    :files ["scripts/check-v3-workstation-config-isomorphism.mjs"
            "scripts/check-v3-code-isomorphism-complete.mjs"
            "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"]
    :summary "Acceptance: workstation-config isomorphism (--dry-fixture and live) OK, V3 aggregate gate OK (7 surfaces / 8 per-surface checkers), cargo check -p missiond-daemon clean, 34/34 autopilot unit tests pass (3 new: owned_dispatch_guard_allows_concurrent_different_slots, owned_dispatch_guard_preserves_same_slot_exclusion, dispatch_board_tasks_uses_concurrent_send_pipeline)."))
