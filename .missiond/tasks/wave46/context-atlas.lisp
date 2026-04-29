;; Wave 46 dispatch-time context atlas.

(context-atlas wave46-v3-internal-execute-dry-run-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave46
  :goal "Make mission_request --execute-dry-run exercise execute_mode=internal and return workstation-dispatch dry-run proof."
  :read-order [".missiond/claudecode/wave46-shared-preamble.md"
               ".missiond/tasks/wave46/context-atlas.lisp"
               ".missiond/tasks/wave46/pattern-cards.lisp"
               ".missiond/tasks/wave46/wave46-01-request-internal-execute-dry-run-v0.lisp"
               ".missiond/tasks/wave45/reports/wave45-01-request-execute-dry-run-smoke-v0.report.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               "scripts/check-v3-request-flow-smoke.mjs"
               "crates/missiond-daemon/src/handlers/knowledge/request.rs"
               "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
               "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
               "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
               "crates/missiond-mcp/src/tools/knowledge/request.rs"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "Architecture authority. Refine execute-dry-run-smoke before checker/code edits."
      :grep ["execute-dry-run-smoke"
             "execute_mode"
             "workstation_dispatch_status"
             "workstation-config"
             "checks"])
    (file "scripts/check-v3-request-flow-smoke.mjs"
      :purpose "Primary target: make --execute-dry-run use execute_mode=internal and assert the workstation-dispatch dry-run substrate."
      :grep ["execute_dry_run_requested"
             "execute_plan_dry_run"
             "pipeline_result"
             "no_dispatch_proof"
             "compat_write_audit"])
    (file "crates/missiond-daemon/src/handlers/knowledge/request.rs"
      :purpose "mission_request adapter. Inspect only unless execute_mode/dry_run/dispatch_strategy is not forwarded."
      :grep ["RespondDecision::ExecutePlan"
             "\"execute_mode\""
             "\"dispatch_strategy\""
             "\"dry_run\""
             "tool_result_payload"])
    (file "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
      :purpose "s6 routing from approved_plan_id + execute=true into mission_plan execute."
      :grep ["build_plan_execute_args"
             "\"execute_mode\""
             "\"dispatch_strategy\""
             "\"dry_run\""
             "plan_execute"])
    (file "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :purpose "mission_plan execute internal dry-run behavior. Current parent probe saw status=dry_run, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dry_run_no_dispatch."
      :grep ["execute_mode=internal"
             "action_execute_internal"
             "workstation_dispatch_v0"
             "dry_run_no_dispatch"
             "run_workstation_dispatch_with_contract_and_trace"])
    (file "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
      :purpose "Substrate response shape for dry-run no-dispatch and task_brief_preview."
      :grep ["WorkstationDispatchOutcome::DryRun"
             "dry_run_no_dispatch"
             "task_brief_preview"
             "mission_task_delegate"])))
