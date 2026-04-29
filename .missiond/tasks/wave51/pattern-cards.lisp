;; Wave 51 dispatch-time pattern cards.

(pattern-cards wave51-concurrent-slot-dispatch-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave51

  (card split-prepare-from-send
    :use-for [wave51-01-autopilot-concurrent-slot-dispatch-v0]
    :summary "Do not await one long pty.send while still selecting and preparing other ready tasks for different slots."
    :recipe ["Keep task selection, dependency checks, prompt assembly, claim, lease, prompt snapshot, and dispatch event semantics intact."
             "Move the send/completion tail into a helper or collectable future so multiple different-slot sends can start in the same dispatch tick."
             "Hold the per-slot RAII guard across each individual send; same-slot work must still be exclusive."
             "Do not change flow task behavior unless needed for compile hygiene."]
    :known-good ["different slot ids can both acquire SlotDispatchGuard"
                 "send futures/jobs are collected before awaiting completion"])

  (card lisp-code-isomorphism-pin
    :use-for [wave51-01-autopilot-concurrent-slot-dispatch-v0]
    :summary "Every runtime invariant change must land with its V3 blueprint text and checker pin."
    :recipe ["Update .missiond/v3/missiond-blueprint.lisp workstation-config invariant/note."
             "Update scripts/check-v3-workstation-config-isomorphism.mjs requireAll + dry fixture."
             "Add a focused regression guard in autopilot tests if a pure/source-level assertion is more practical than a DB-backed integration test."
             "Run the per-surface checker before the aggregate V3 gate."]
    :known-good ["node scripts/check-v3-workstation-config-isomorphism.mjs"
                 "node scripts/check-v3-code-isomorphism-complete.mjs"])
)
