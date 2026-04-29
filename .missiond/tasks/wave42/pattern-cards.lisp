;; Wave 42 dispatch-time pattern cards.

(pattern-cards wave42-v3-request-flow-smoke-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave42

  (card executable-user-flow-smoke
    :use-for [wave42-01-v3-request-flow-smoke-v0]
    :summary "A complete V3 gate should prove the user-facing request flow, not only individual surface needles."
    :recipe ["Model the exact review states from Lisp artifacts: request received, awaiting intent approval, awaiting plan approval, awaiting execution, execute requested."
             "Use deterministic dry fixtures with temp request directories so the checker is CI-safe."
             "Make the checker fail when the plan approval gate can no longer be derived from plan.lisp plus approve_plan events."
             "Keep real workstation dispatch out of the default smoke; the execute step should verify the gate/intent, not consume a slot."]
    :known-good ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
                 "scripts/check-v3-task-lifecycle-isomorphism.mjs"])

  (card lisp-first-gap-fix
    :use-for [wave42-01-v3-request-flow-smoke-v0]
    :summary "If the smoke exposes drift, update the V3 Lisp contract first, then code/checkers to match it."
    :recipe ["Do not silently patch Rust around an unclear contract."
             "If behavior is already correct but unpinned, add the checker and a concise blueprint note/check entry."
             "If behavior is wrong, adjust the blueprint wording first, then repair request.rs or MCP schema within this task's write scope."
             "Preserve the two-gate human flow: approve_intent creates/reaches plan.lisp, approve_plan reaches awaiting_execution, execute_plan is explicit."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"
                 "scripts/check-v3-request-lisp-isomorphism.mjs"])

  (card no-slot-consuming-live-smoke
    :use-for [wave42-01-v3-request-flow-smoke-v0]
    :summary "The default request-flow checker must not dispatch a real workstation task."
    :recipe ["Default mode should be read-only/static plus fixture validation."
             "If an optional --live-ipc mode is added, make it opt-in, clearly named, and stop before real execution unless an additional explicit flag is supplied."
             "Never call mission_task_delegate from this checker in default acceptance."]
    :known-good ["scripts/task-runner-submit-dispatch.mjs"
                 "crates/missiond-mcp/src/tools/knowledge/request.rs"]))
