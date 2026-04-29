;; Wave 46 dispatch-time pattern cards.

(pattern-cards wave46-v3-internal-execute-dry-run-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave46

  (card internal-dry-run-before-real-dispatch
    :use-for [wave46-01-request-internal-execute-dry-run-v0]
    :summary "Before a smoke may real-dispatch a workstation task, prove execute_mode=internal reaches the dispatch substrate with dry_run=true."
    :recipe ["Default --live-ipc remains non-executing and stops at awaiting_execution."
             "--execute-dry-run passes execute=true, execute_mode=internal, dry_run=true, target=mission_task_delegate, dispatch_strategy=agent-team."
             "Assert request/review invariants first: execute_requested, observe-only continuation, and execute_plan review event."
             "Then assert substrate invariants: status=dry_run, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dry_run_no_dispatch, task_brief_preview present."
             "Do not spawn or wait for a ClaudeCode worker from this checker."]
    :known-good ["scripts/check-v3-request-flow-smoke.mjs"
                 "crates/missiond-daemon/src/handlers/knowledge/plan.rs"])

  (card lisp-first-before-checker
    :use-for [wave46-01-request-internal-execute-dry-run-v0]
    :summary "V3 blueprint is the contract; checker and code are projections."
    :recipe ["Update .missiond/v3/missiond-blueprint.lisp first with the internal execute dry-run contract."
             "Then update the JS checker."
             "Only touch Rust or MCP if live smoke reveals forwarding/schema drift."
             "Run aggregate v3 gates after the live internal execute smoke."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"
                 "scripts/check-v3-code-isomorphism-complete.mjs"])

  (card request-local-cleanup-stays-narrow
    :use-for [wave46-01-request-internal-execute-dry-run-v0]
    :summary "The request smoke may create DB audit rows, but filesystem cleanup remains request-local only."
    :recipe ["Use a unique request_id in live acceptance."
             "After --cleanup, assert no .missiond/requests/<request_id> directory remains."
             "Do not delete database rows or legacy roots."
             "Keep compat_write_audit intact so default flow does not leak compatibility files."]
    :known-good ["scripts/check-v3-request-flow-smoke.mjs"]))
