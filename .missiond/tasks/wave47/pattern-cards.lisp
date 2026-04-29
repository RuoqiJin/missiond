;; Wave 47 dispatch-time pattern cards.

(pattern-cards wave47-v3-request-real-dispatch-smoke-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave47

  (card explicit-real-dispatch-only
    :use-for [wave47-01-request-real-dispatch-smoke-v0]
    :summary "Real dispatch is slow and side-effecting; it must be behind a separate flag and absent from aggregate checks."
    :recipe ["Keep default --live-ipc stopping at awaiting_execution."
             "Keep --execute-dry-run as wave46's no-slot substrate proof."
             "Add --execute-real-dispatch as a separate, obvious opt-in; do not overload --confirm-execute."
             "Do not add the real-dispatch flag to check-v3-code-isomorphism-complete."
             "If waiting for Autopilot is supported, gate it behind a second bounded option."]
    :known-good ["scripts/check-v3-request-flow-smoke.mjs"
                 ".missiond/v3/missiond-blueprint.lisp"])

  (card lisp-first-before-checker
    :use-for [wave47-01-request-real-dispatch-smoke-v0]
    :summary "V3 blueprint is the contract; checker and code are projections."
    :recipe ["Add the blueprint real-dispatch-smoke section first."
             "Then update checker flags, usage text, fixtures, and assertions."
             "Only touch Rust/MCP if live real dispatch exposes missing response fields or schema forwarding drift."
             "Report why any Rust/MCP change was or was not needed."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"
                 "scripts/check-v3-code-isomorphism-complete.mjs"])

  (card delegated-smoke-must-be-no-edit
    :use-for [wave47-01-request-real-dispatch-smoke-v0]
    :summary "The delegated BoardTask created by the smoke should prove dispatch without editing the repository."
    :recipe ["Use a smoke objective that tells the delegated worker: do not edit files, do not commit, run git status --short, return a concise summary."
             "The checker should assert creation/response shape and report the BoardTask id/status."
             "--cleanup removes only request-local files; delegated BoardTask rows remain observable audit records."
             "Parent/orchestrator observes or closes the smoke BoardTask after the checker returns."]
    :known-good ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                 "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"]))
