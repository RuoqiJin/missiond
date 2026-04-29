;; Wave 49 dispatch-time pattern cards.

(pattern-cards wave49-restart-recovery-smoke-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave49

  (card opt-in-live-smoke
    :use-for [wave49-01-request-flow-restart-recovery-smoke-v0]
    :summary "Live restart behavior must be behind explicit flags; default and dry modes stay safe."
    :recipe ["Keep default invocation read-only."
             "Reject --restart-during-dispatch unless --live-ipc and --execute-real-dispatch are also present."
             "Dry fixtures should exercise parser/planner branches without touching a daemon."
             "Report the exact parent-run live command, but do not run it unless explicitly asked."]
    :known-good ["scripts/check-v3-request-flow-smoke.mjs --dry-fixture"
                 "scripts/check-v3-request-flow-smoke.mjs"])

  (card single-file-js-surgery
    :use-for [wave49-01-request-flow-restart-recovery-smoke-v0]
    :summary "Make a localized JS change; avoid broad refactors and unrelated checker churn."
    :recipe ["Read the existing CLI flag parsing and fixture helper before editing."
             "Add small helpers for restart-smoke planning instead of restructuring the full script."
             "Run the script's dry fixture immediately after each meaningful edit."
             "Run perl NUL audit on the edited script before commit."]
    :known-good ["scripts/check-v3-request-flow-smoke.mjs"])
)
