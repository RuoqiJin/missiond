;; Wave 45 dispatch-time pattern cards.

(pattern-cards wave45-v3-request-execute-dry-run-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave45

  (card execute-dry-run-is-smoke-not-dispatch
    :use-for [wave45-01-request-execute-dry-run-smoke-v0]
    :summary "A smoke checker may prove execute_plan routing only when it is explicitly no-dispatch."
    :recipe ["Default --live-ipc must keep stopping at awaiting_execution."
             "Add a separate --execute-dry-run flag; pass execute=true and dry_run=true only under that flag."
             "Assert review_packet.state=execute_requested and allowed_responses=[observe]."
             "Accept runner_status=bridge_only/status=bridge_ready or runner_status=dry_run_no_dispatch/status=dry_run as no-dispatch proof."
             "Do not spawn or wait for a ClaudeCode worker from this checker."]
    :known-good ["scripts/check-v3-request-flow-smoke.mjs"
                 "crates/missiond-daemon/src/handlers/knowledge/request.rs"])

  (card request-local-cleanup-stays-narrow
    :use-for [wave45-01-request-execute-dry-run-smoke-v0]
    :summary "The request smoke may create DB audit rows, but filesystem cleanup remains request-local only."
    :recipe ["Use a unique request_id in live acceptance."
             "After --cleanup, assert no .missiond/requests/<request_id> directory remains."
             "Do not delete database rows or legacy roots."
             "Keep wave44 compat_write_audit intact so default flow does not leak compatibility files."]
    :known-good ["scripts/check-v3-request-flow-smoke.mjs"])

  (card lisp-first-before-checker
    :use-for [wave45-01-request-execute-dry-run-smoke-v0]
    :summary "V3 blueprint is the contract; checker and code are projections."
    :recipe ["Update .missiond/v3/missiond-blueprint.lisp first with the execute dry-run smoke contract."
             "Then update the JS checker."
             "Only touch Rust or MCP if the checker reveals behavior/schema drift."
             "Run aggregate v3 gates after the live execute smoke."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"
                 "scripts/check-v3-code-isomorphism-complete.mjs"]))
