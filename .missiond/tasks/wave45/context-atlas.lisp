;; Wave 45 dispatch-time context atlas.

(context-atlas wave45-v3-request-execute-dry-run-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave45
  :goal "Make mission_request execute_plan dry-run observable from request-local plan.lisp without workstation dispatch."
  :read-order [".missiond/claudecode/wave45-shared-preamble.md"
               ".missiond/tasks/wave45/context-atlas.lisp"
               ".missiond/tasks/wave45/pattern-cards.lisp"
               ".missiond/tasks/wave45/wave45-01-request-execute-dry-run-smoke-v0.lisp"
               ".missiond/tasks/wave44/reports/wave44-01-request-local-artifact-roots-v0.report.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               "scripts/check-v3-request-flow-smoke.mjs"
               "crates/missiond-daemon/src/handlers/knowledge/request.rs"
               "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
               "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
               "crates/missiond-mcp/src/tools/knowledge/request.rs"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "Architecture authority. Add the explicit live execute dry-run smoke invariant before checker/code edits."
      :grep ["execute-plan"
             "plan_approved -> :executing"
             "surface mission_request"
             "compat-writer-policy"
             "checks"])
    (file "scripts/check-v3-request-flow-smoke.mjs"
      :purpose "Primary target: add --execute-dry-run live IPC step after approve_plan while default remains non-executing."
      :grep ["runLiveIpcSmoke"
             "confirm_execute"
             "approve_plan"
             "compat_write_audit"
             "execute_requested"])
    (file "crates/missiond-daemon/src/handlers/knowledge/request.rs"
      :purpose "mission_request adapter. Inspect only unless execute dry-run response/state is wrong."
      :grep ["RespondDecision::ExecutePlan"
             "unified_entry::plan_execute"
             "derive_review_packet"
             "build_review_event_lisp"])
    (file "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
      :purpose "s6 routing from approved_plan_id + execute=true into mission_plan execute."
      :grep ["build_plan_execute_args"
             "PipelineDecision::PlanExecute"
             "\"dry_run\""
             "runner_status"])
    (file "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :purpose "mission_plan execute payload shapes; no-dispatch proof currently may be bridge_ready/bridge_only or dry_run/dry_run_no_dispatch."
      :grep ["bridge_ready"
             "bridge_only"
             "dry_run_no_dispatch"
             "action_execute_internal"])))
