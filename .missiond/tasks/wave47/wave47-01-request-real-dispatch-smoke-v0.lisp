;; Wave 47 task contract.

(task wave47-01-request-real-dispatch-smoke-v0
  :schema "missiond.task-contract.v1"
  :title "v3 request real dispatch smoke v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier smoke
  :dispatch-group "A"
  :estimated-minutes 70
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave47/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave47/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Wave46 proved mission_request execute_plan can enter the internal workstation-dispatch substrate in dry_run mode. Add the next explicit audit layer: an opt-in real-dispatch smoke that drives mission_request start -> approve_intent -> approve_plan -> execute_plan with execute_mode=internal, dispatch_strategy=agent-team, target=mission_task_delegate, dry_run=false, and proves the response creates a delegated BoardTask through the workstation-dispatch substrate. Keep this slow side-effecting smoke out of default --live-ipc and out of the aggregate code-isomorphism gate."

  :write-scope
    [".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-v3-request-flow-smoke.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/request.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
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
     ".missiond/tasks/wave43/**"
     ".missiond/tasks/wave44/**"
     ".missiond/tasks/wave45/**"
     ".missiond/tasks/wave46/**"
     ".missiond/tasks/wave47/manifest.lisp"
     ".missiond/tasks/wave47/context-atlas.lisp"
     ".missiond/tasks/wave47/pattern-cards.lisp"
     ".missiond/tasks/wave47/wave47-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Update .missiond/v3/missiond-blueprint.lisp first. Add a real-dispatch-smoke sibling under unified-entry that explicitly declares the audit is opt-in only, side-effecting, and excluded from default --live-ipc plus check-v3-code-isomorphism-complete."
     "Extend scripts/check-v3-request-flow-smoke.mjs with a deliberately named opt-in flag, preferred name --execute-real-dispatch. Do not overload --confirm-execute. Preserve --execute-dry-run semantics from wave46 unchanged."
     "The real-dispatch flag must call mission_request respond with response=execute_plan, execute=true, dry_run=false or omitted, execute_mode='internal', dispatch_strategy='agent-team', target='mission_task_delegate', cwd=<repo>, and a smoke objective that tells the delegated worker to do no file edits and no commits."
     "Assert the same review-level invariants as wave45/46: respond outcome=dispatched, inner_action=unified_entry::plan_execute, respond_result.execute=true, review_packet.state=execute_requested, allowed_responses=[observe], and a request-local execute_plan event is appended."
     "Assert real-dispatch substrate invariants: pipeline_result.status='dispatched', pipeline_result.execute_mode='internal', pipeline_result.runner_status='workstation_dispatch_v0', pipeline_result.workstation_dispatch_status='dispatched', pipeline_result.target_tool='mission_task_delegate', pipeline_result.dispatch_strategy='agent-team', task_brief_preview present, and inner_result present."
     "Extract a delegated BoardTask identifier from the inner_result or response payload if the current daemon exposes one. If the payload does not expose a stable task id, fail with a diagnostic that names the missing field and update the Rust response projection minimally to surface it from the existing mission_task_delegate result; keep this Lisp-first."
     "If the checker offers a wait/observe mode, it must be separately gated and bounded. The required acceptance for this task may validate only creation/response shape and may leave the delegated no-edit smoke BoardTask for Autopilot to finish; do not make normal CI wait on ClaudeCode."
     "Cleanup remains request-local for files: --cleanup may remove only .missiond/requests/<request_id>/. It must not delete DB audit rows or the delegated BoardTask. Report the BoardTask id/status so the parent can observe/close it."
     "Keep default daemon-free behavior, default --live-ipc, --execute-dry-run, and aggregate v3 gate non-real-dispatching. The new real dispatch path must require its own explicit flag."]

  :acceptance
    ["node scripts/check-v3-request-flow-smoke.mjs --dry-fixture"
     "node scripts/check-v3-request-flow-smoke.mjs"
     "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave47-request-real-dispatch-v0 --cleanup"
     "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave47-request-real-dispatch-v0-dry --cleanup --execute-dry-run"
     "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave47-request-real-dispatch-v0-dry-json --cleanup --execute-dry-run --json"
     "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave47-request-real-dispatch-v0-real --cleanup --execute-real-dispatch --json"
     "node scripts/check-v3-code-isomorphism-complete.mjs"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "cargo test -p missiond-daemon handlers::knowledge::request::tests"
     "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered"
     "perl -ne 'exit 1 if /\\x00/' scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-mcp/src/tools/knowledge/request.rs"
     "git diff --check -- scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-mcp/src/tools/knowledge/request.rs"]

  :commit
    (:required true
     :message "feat(v3): add request real dispatch smoke"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Exact real-dispatch response shape observed in pipeline_result, including status, runner_status, workstation_dispatch_status, target_tool, dispatch_strategy, task_brief_preview, and inner_result."
     "Delegated BoardTask id and observed status if exposed."
     "Whether Rust/MCP changed, or why blueprint + checker were sufficient."
     "Proof that default --live-ipc, --execute-dry-run, and aggregate gates do not real-dispatch."
     "Whether request-local cleanup left any filesystem artifacts."
     "Acceptance command results."])
