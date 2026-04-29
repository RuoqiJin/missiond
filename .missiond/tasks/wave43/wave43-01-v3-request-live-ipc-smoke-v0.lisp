;; Wave 43 task contract.

(task wave43-01-v3-request-live-ipc-smoke-v0
  :schema "missiond.task-contract.v1"
  :title "v3 request live ipc smoke v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier smoke
  :dispatch-group "A"
  :estimated-minutes 55
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave43/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave43/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Upgrade the wave42 request-flow smoke from static Lisp/code fixtures to an opt-in live IPC smoke that calls the running MissionD daemon through mission_request, produces real request-local intent-alignment.lisp and plan.lisp artifacts, approves intent and plan through the public unified entry, and stops at awaiting_execution without dispatching a workstation task."

  :write-scope
    ["scripts/check-v3-request-flow-smoke.mjs"
     ".missiond/v3/missiond-blueprint.lisp"
     "crates/missiond-daemon/src/handlers/knowledge/request.rs"
     "crates/missiond-mcp/src/tools/knowledge/request.rs"]

  :must-not-touch
    ["packages/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/router/**"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/**"
     ".missiond/tasks/wave30/**"
     ".missiond/tasks/wave31/**"
     ".missiond/tasks/wave32/**"
     ".missiond/tasks/wave33/**"
     ".missiond/tasks/wave34/**"
     ".missiond/tasks/wave35/**"
     ".missiond/tasks/wave36/**"
     ".missiond/tasks/wave37/**"
     ".missiond/tasks/wave38/**"
     ".missiond/tasks/wave39/**"
     ".missiond/tasks/wave40/**"
     ".missiond/tasks/wave41/**"
     ".missiond/tasks/wave42/**"
     ".missiond/tasks/wave43/manifest.lisp"
     ".missiond/tasks/wave43/context-atlas.lisp"
     ".missiond/tasks/wave43/pattern-cards.lisp"
     ".missiond/tasks/wave43/wave43-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Extend scripts/check-v3-request-flow-smoke.mjs with a real --live-ipc mode. The existing default mode and --dry-fixture behavior must remain daemon-free and must still pass unchanged."
     "Reuse scripts/task-runner-submit-dispatch.mjs callToolViaIpc or an equivalent existing helper; do not duplicate raw socket protocol unless the helper cannot be imported cleanly."
     "The live smoke should call mission_request action=start with a wave43-live-ipc-smoke-* request_id, project/cwd pointing at this repo, compiler_mode=dry_run, persist=true, write_request_file=true, write_file=true, overwrite_file=true, review_gate_policy=manual, and a harmless objective. It must assert request.lisp and intent-alignment.lisp exist and the returned/re-polled review_packet is awaiting_intent_approval."
     "Then call mission_request action=respond response=approve_intent for the same request. It must assert plan.lisp exists, contains executable routing hints, and the review_packet is awaiting_plan_approval."
     "Then call mission_request action=respond response=approve_plan. It must assert plan.lisp is stamped with :plan_id, :version, and :board_task_id, and the review_packet is awaiting_execution with execute_allowed=true and allowed_responses containing execute_plan."
     "Do not call execute_plan in the live smoke acceptance path. The point is to prove the execution gate, not to dispatch a workstation slot."
     "Support --endpoint, --session-id, --request-id, --cleanup, and --json. --cleanup may remove only .missiond/requests/<request_id> after validation; document that DB rows may remain as audit records."
     "If live IPC exposes Lisp/code drift, update .missiond/v3/missiond-blueprint.lisp first, then fix request.rs or MCP schema surgically. If no drift is found, avoid Rust edits."]

  :acceptance
    ["node scripts/check-v3-request-flow-smoke.mjs --dry-fixture"
     "node scripts/check-v3-request-flow-smoke.mjs"
     "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave43-live-ipc-smoke-v0 --cleanup"
     "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave43-live-ipc-smoke-v0-json --cleanup --json"
     "node scripts/check-v3-code-isomorphism-complete.mjs"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "cargo test -p missiond-daemon handlers::knowledge::request::tests"
     "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered"
     "perl -ne 'exit 1 if /\\x00/' scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs"
     "git diff --check -- scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs"]

  :commit
    (:required true
     :message "feat(v3): add request live-ipc smoke"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Live IPC call sequence and validated artifacts/states."
     "Whether Lisp/code drift was found."
     "Side effects and cleanup behavior."
     "Acceptance command results."])
