;; Wave 47 task report.
;; Schema: missiond.report-contract.v1

(report wave47-01-request-real-dispatch-smoke-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave47-01-request-real-dispatch-smoke-v0"
  :status done
  :commit_hash "75f0791ce096"
  :files_changed
    [".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-v3-request-flow-smoke.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
  :acceptance_results
    [(:command "node scripts/check-v3-request-flow-smoke.mjs --dry-fixture"
              :exit_code 0 :ok true
              :note "Daemon-free fixture-only mode unchanged: 9 fixtures, 6 states, 6 decisions.")
     (:command "node scripts/check-v3-request-flow-smoke.mjs"
              :exit_code 0 :ok true
              :note "Default static + fixture mode unchanged and daemon-free.")
     (:command "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave47-request-real-dispatch-v0 --cleanup"
              :exit_code 0 :ok true
              :note "Default --live-ipc still stops at awaiting_execution: 4 live steps OK. No execute_plan call and no real workstation dispatch.")
     (:command "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave47-request-real-dispatch-v0-dry --cleanup --execute-dry-run"
              :exit_code 0 :ok true
              :note "Wave46 no-slot audit still passes: execute_plan_dry_run returns status=dry_run, execute_mode=internal, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dry_run_no_dispatch, target_tool=mission_task_delegate, dispatch_strategy=agent-team, task_brief_preview present.")
     (:command "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave47-request-real-dispatch-v0-dry-json --cleanup --execute-dry-run --json"
              :exit_code 0 :ok true
              :note "JSON dry-run proof pinned: respond_outcome=dispatched, inner_action=unified_entry::plan_execute, review_packet.state=execute_requested, allowed_responses=[observe], request-local execute_plan event appended, no_dispatch_proof=true.")
     (:command "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave47-request-real-dispatch-v0-real --cleanup --execute-real-dispatch --json"
              :exit_code 0 :ok true
              :note "Opt-in real-dispatch proof passed. Observed pipeline_result: status=executing, execute_mode=internal, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dispatched, target_tool=mission_task_delegate, dispatch_strategy=agent-team, task_brief_preview present, inner_result present, delegated_board_task_id=1223f053-0563-49b6-b9a6-e169ee2830eb, delegated_board_task_status=open, delegated_board_task_assignee=slot-dyn-2badc912, dispatch_proof=true. Parent closed that no-edit audit BoardTask after observing the proof.")
     (:command "node scripts/check-v3-code-isomorphism-complete.mjs"
              :exit_code 0 :ok true
              :note "Aggregate gate remains daemon-free and non-real-dispatching: 6 surfaces graduated, 7 per-surface checkers passed. It does not pass --execute-real-dispatch.")
     (:command "node scripts/check-lisp-blueprint-compression.mjs"
              :exit_code 0 :ok true
              :note "Blueprint compression still holds with the new real-dispatch-smoke sibling under unified-entry.")
     (:command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
              :exit_code 0 :ok true
              :note "Architecture Lisp check OK on the updated V3 blueprint.")
     (:command "cargo test -p missiond-daemon handlers::knowledge::request::tests"
              :exit_code 0 :ok true
              :note "86 request handler tests pass. Existing request/unified_entry/plan forwarding path was sufficient; only workstation_dispatch.rs needed a response projection for delegated_board_task_id.")
     (:command "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered"
              :exit_code 0 :ok true
              :note "MCP surface registry smoke passes; mission_request schema remains registered.")
     (:command "perl -ne 'exit 1 if /\\x00/' scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-mcp/src/tools/knowledge/request.rs"
              :exit_code 0 :ok true
              :note "No NUL bytes in the audited write-scope files.")
     (:command "git diff --check -- scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-mcp/src/tools/knowledge/request.rs"
              :exit_code 0 :ok true
              :note "No whitespace errors in the audited write-scope files.")]
  :notes "Wave47 adds a deliberately opt-in --execute-real-dispatch audit to the V3 request-flow smoke. The default live path and --execute-dry-run path remain non-real-dispatching; the aggregate code-isomorphism gate remains daemon-free and never passes the new flag.\n\nImportant live-shape correction: the task draft expected pipeline_result.status='dispatched', but the established plan FSM returns status='executing' after a successful WorkstationDispatchOutcome::Dispatched. The substrate-level dispatch invariant is pipeline_result.workstation_dispatch_status='dispatched'. The V3 blueprint and checker now pin this exact split: status=executing plus runner_status=workstation_dispatch_v0 plus workstation_dispatch_status=dispatched.\n\nRust/MCP scope: request.rs, unified_entry.rs, plan.rs, and missiond-mcp request schema did not require changes. The forwarding path already carries execute_mode=internal, dispatch_strategy=agent-team, dry_run=false, target=mission_task_delegate, cwd, and objective into mission_plan execute. The only Rust change is workstation_dispatch.rs::extract_inner_board_task_id plus a top-level delegated_board_task_id projection, because mission_task_delegate currently serializes the full BoardTask row under inner_result.task_id rather than a bare UUID.\n\nFinal real-dispatch response proof from /tmp/wave47-real-json-final.out: respond_outcome=dispatched, inner_action=unified_entry::plan_execute, respond_result.execute=true, review_packet.state=execute_requested, allowed_responses=[observe], execute event 000004 appended, pipeline_status=executing, pipeline_runner_status=workstation_dispatch_v0, pipeline_execute_mode=internal, pipeline_workstation_dispatch_status=dispatched, pipeline_target_tool=mission_task_delegate, pipeline_dispatch_strategy=agent-team, task_brief_preview present, inner_result present, delegated_board_task_id=1223f053-0563-49b6-b9a6-e169ee2830eb.\n\nOperational note: the initial ClaudeCode worker lost its dynamic slot when the daemon had to be restarted to install the new workstation_dispatch projection. Parent took over without discarding worker edits, installed the rebuilt daemon binary at /Users/jinchen/.xjp-mission/missiond, reran the live smokes, closed the failed wave47 BoardTask via mission_board_update, and closed the no-edit audit BoardTasks created by real-dispatch smokes after observing their proof. Request-local --cleanup removed .missiond/requests/<request_id>/; temporary .missiond/v2/plans/*.evidence.json sidecars from the smoke runs were manually removed from the worktree after report capture so the commit stays scoped."
  :verification_tier smoke)
