;; Wave 47 dispatch-time context atlas.

(context-atlas wave47-v3-request-real-dispatch-smoke-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave47
  :goal "Make mission_request execute_plan real dispatch observable through an explicit opt-in smoke."
  :read-order [".missiond/claudecode/wave47-shared-preamble.md"
               ".missiond/tasks/wave47/context-atlas.lisp"
               ".missiond/tasks/wave47/pattern-cards.lisp"
               ".missiond/tasks/wave47/wave47-01-request-real-dispatch-smoke-v0.lisp"
               ".missiond/tasks/wave46/reports/wave46-01-request-internal-execute-dry-run-v0.report.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               "scripts/check-v3-request-flow-smoke.mjs"
               "crates/missiond-daemon/src/handlers/knowledge/request.rs"
               "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
               "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
               "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
               "crates/missiond-mcp/src/tools/knowledge/request.rs"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "Architecture authority. Add real-dispatch-smoke before checker/code edits."
      :grep ["execute-dry-run-smoke"
             "real-dispatch"
             "workstation_dispatch_status"
             "mission_task_delegate"
             "checks"])
    (file "scripts/check-v3-request-flow-smoke.mjs"
      :purpose "Primary target: add --execute-real-dispatch while preserving default and --execute-dry-run behavior."
      :grep ["execute_plan_dry_run"
             "executeDryRun"
             "confirm_execute"
             "pipeline_result"
             "compat_write_audit"
             "cleanup"])
    (file "crates/missiond-daemon/src/handlers/knowledge/request.rs"
      :purpose "mission_request adapter. Inspect or touch only if execute_plan real-dispatch response lacks the needed pipeline_result fields."
      :grep ["RespondDecision::ExecutePlan"
             "tool_result_payload"
             "\"execute_mode\""
             "\"dispatch_strategy\""
             "\"dry_run\""])
    (file "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
      :purpose "s6 routing from approved_plan_id + execute=true into mission_plan execute."
      :grep ["build_plan_execute_args"
             "plan_execute"
             "\"execute_mode\""
             "\"dispatch_strategy\""
             "\"dry_run\""])
    (file "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :purpose "mission_plan execute internal response shape; build_workstation_dispatch_response adds runner_status and target_tool."
      :grep ["build_workstation_dispatch_response"
             "workstation_dispatch_v0"
             "target_tool"
             "inner_result"
             "WorkstationDispatchOutcome::Dispatched"])
    (file "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
      :purpose "Substrate real-dispatch response shape and inner mission_task_delegate payload."
      :grep ["WorkstationDispatchOutcome::Dispatched"
             "outcome_to_response_fields"
             "inner_result"
             "mission_task_delegate"
             "task_brief_preview"])))
