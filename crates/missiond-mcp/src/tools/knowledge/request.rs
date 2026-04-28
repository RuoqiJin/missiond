use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_request",
        "MissionD v3 unified request entry — request-first surface for user/external/agent needs. \
         v0 is file-first + compatibility composition: action=start writes \
         .missiond/requests/<request_id>/request.lisp + initial lifecycle event, forwards to the \
         existing unified-entry pipeline (mission_directive / mission_plan), and projects the \
         pipeline's stable inner sexp into request-local Lisp artifacts \
         (.missiond/requests/<request_id>/intent-alignment.lisp from a directive compile, \
         plan.lisp from a plan compile). action=advance forwards an already-approved directive/plan \
         breadcrumb to the same pipeline and runs the same projection. action=status reads \
         request.lisp and surfaces request-local artifact paths + existence booleans. action=respond \
         answers a review_packet: callers pass response (or decision) = approve_intent | reject_intent | \
         ask_question | approve_plan | reject_plan | execute_plan along with the request_id and an \
         optional note. mission_request resolves the persisted directive/plan ref from explicit args \
         (approved_directive_id / directive_id + directive_version, approved_plan_id / plan_id) or \
         from the request-local intent-alignment.lisp / plan.lisp; missing refs return a structured \
         blocked response with next_action instead of fabricating an id. approve_intent delegates to \
         mission_directive(action=approve); approve_plan delegates to mission_plan(action=approve) \
         and never sets execute=true; execute_plan requires execute=true (or response=execute_plan) \
         and routes through the existing mission_plan execute path via unified_entry. reject_intent / \
         reject_plan / ask_question never mutate directive/plan approval state and only append a \
         request-local review event under .missiond/requests/<request_id>/events/<seq>.event.lisp. \
         All actions never auto-approve intent or plan, never bypass mission_plan, and never spawn \
         workstation work directly. Wrapper response shape (start/advance): { status, action, mode, \
         request_artifacts, projection: { status: written|skipped_*|write_failed, target?, \
         sexp_source?, path?, sha256?, bytes?, created?, overwritten?, error? }, pipeline, v3_contract, \
         next_step, review_packet?: { state: received|intent_drafting|awaiting_intent_approval|\
         awaiting_plan_approval|execute_requested, artifact_kind: request|intent_alignment|plan, \
         artifact_path, artifact_exists, artifact_preview (UTF-8-safe truncation, ≤480 bytes), \
         prompt, allowed_responses, next_action, execute_allowed } }. respond response shape: \
         { status: ok|blocked, action: respond, mode, request_id, request_path, artifact_paths, \
         artifact_exists, respond_result: { decision, outcome: recorded|dispatched|blocked, \
         event_path, event_seq, event_sha256, event_bytes, execute, next_action, directive_id?, \
         directive_version?, plan_id?, inner_action?, blocked_reason?, note? }, review_packet, \
         next_action, v3_contract, pipeline_result? }. review_packet is a pure projection of \
         request-local artifact existence + the latest projection target — the caller decides \
         whether to approve via mission_directive/mission_plan; mission_request never silently \
         approves or dispatches.",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "advance", "status", "respond"],
                    "description": "start=create/request artifact + run next unified pipeline stage; advance=run next stage with approved_directive_id or approved_plan_id; status=read request.lisp; respond=answer a review_packet (approve_intent | reject_intent | ask_question | approve_plan | reject_plan | execute_plan) — delegates approve/execute to mission_directive/mission_plan and records review events under .missiond/requests/<request_id>/events"
                },
                "response": {
                    "type": "string",
                    "enum": [
                        "approve_intent",
                        "reject_intent",
                        "ask_question",
                        "approve_plan",
                        "reject_plan",
                        "execute_plan"
                    ],
                    "description": "[respond] review decision — approve_intent dispatches to mission_directive approve; approve_plan dispatches to mission_plan approve (never execute); execute_plan requires execute=true and routes through mission_plan execute; reject_intent/reject_plan/ask_question only append a request-local review event"
                },
                "decision": {
                    "type": "string",
                    "enum": [
                        "approve_intent",
                        "reject_intent",
                        "ask_question",
                        "approve_plan",
                        "reject_plan",
                        "execute_plan"
                    ],
                    "description": "[respond] alias for `response`"
                },
                "note": {
                    "type": "string",
                    "description": "[respond] optional human note recorded in the request-local review event (required in spirit for reject_*/ask_question routes)"
                },
                "message": {
                    "type": "string",
                    "description": "[start] user need / external request body"
                },
                "request_id": {
                    "type": "string",
                    "description": "[start|advance|status|respond] stable request id. Omit on start to allocate req-<uuid-prefix>."
                },
                "mode": {
                    "type": "string",
                    "enum": ["human_interactive", "trusted_agent"],
                    "description": "[start] v3 entry mode. human_interactive keeps both review gates; trusted_agent may fold intent into plan only through policy gates. Default human_interactive."
                },
                "project": {
                    "type": "string",
                    "description": "[start|status|respond] registered project id used to resolve project root for .missiond/requests writes"
                },
                "cwd": {
                    "type": "string",
                    "description": "[start|status|respond] absolute cwd inside a registered project; used when project is omitted"
                },
                "target_project": {
                    "type": "string",
                    "description": "[start|status|respond] fallback project id used when project/cwd are omitted; also forwarded to mission_plan"
                },
                "write_request_file": {
                    "type": "boolean",
                    "description": "[start] default true. When true, writes request.lisp and 000001.event.lisp. Set false for preview-only routing."
                },
                "overwrite_file": {
                    "type": "boolean",
                    "description": "[start|advance] allow replacing an existing request.lisp / initial event AND any request-local intent-alignment.lisp / plan.lisp projection produced from the inner compile sexp. Default false."
                },
                "compiler_mode": {
                    "type": "string",
                    "enum": ["dry_run", "sonnet"],
                    "description": "[start|advance] forwarded to mission_directive / mission_plan compile. Default dry_run on those surfaces."
                },
                "persist": {
                    "type": "boolean",
                    "description": "[start|advance] forwarded to directive/plan compile. Default false on inner surfaces."
                },
                "approved_directive_id": {
                    "type": "string",
                    "description": "[advance|respond approve_intent/reject_intent] approved directive UUID; on advance triggers plan-authoring path; on respond identifies the directive to approve/reject without bypassing mission_directive's gate"
                },
                "directive_id": {
                    "type": "string",
                    "description": "[respond approve_intent/reject_intent] alias for approved_directive_id"
                },
                "directive_version": {
                    "type": "integer",
                    "description": "[advance|respond approve_intent/reject_intent] directive version forwarded to mission_plan compile or mission_directive approve"
                },
                "board_task_id": {
                    "type": "string",
                    "description": "[advance plan-authoring|respond] board task anchor required by mission_plan compile; respond forwards it to follow-up advance calls"
                },
                "approved_plan_id": {
                    "type": "string",
                    "description": "[advance execute|respond approve_plan/reject_plan/execute_plan] approved plan UUID"
                },
                "plan_id": {
                    "type": "string",
                    "description": "[respond approve_plan/reject_plan/execute_plan] alias for approved_plan_id"
                },
                "execute": {
                    "type": "boolean",
                    "description": "[advance execute|respond execute_plan] must be true with approved_plan_id; mission_request never auto-executes on id alone. respond approve_plan ignores this flag — only execute_plan honours it"
                },
                "execute_after_approval": {
                    "type": "boolean",
                    "description": "Alias for execute"
                },
                "topic": {
                    "type": "string",
                    "description": "[start|advance] forwarded to file-first compatibility writers; defaults to request_id when mission_request allocates one"
                },
                "write_file": {
                    "type": "boolean",
                    "description": "[start|advance] forwarded to mission_directive / mission_plan file-first compatibility writer"
                },
                "review_gate_policy": {
                    "type": "string",
                    "enum": ["manual", "emit_question", "off"],
                    "description": "[start|advance] forwarded to directive/plan compile review gate policy"
                },
                "emit_review_question": {
                    "type": "boolean",
                    "description": "[start|advance] forwarded to directive/plan compile review gate"
                },
                "review_question_text": {
                    "type": "string",
                    "description": "[start|advance] forwarded to directive/plan compile review gate"
                },
                "review_question_id": {
                    "type": "string",
                    "description": "[start|advance] forwarded to directive/plan compile/execute surfaces"
                },
                "dispatch_strategy": {
                    "type": "string",
                    "description": "[advance] forwarded to mission_plan compile/execute"
                },
                "parallelism": {
                    "type": "string",
                    "description": "[advance] forwarded to mission_plan compile"
                },
                "target": {
                    "type": "string",
                    "description": "[advance execute] forwarded to mission_plan execute"
                },
                "execute_mode": {
                    "type": "string",
                    "enum": ["bridge", "internal"],
                    "description": "[advance execute] forwarded to mission_plan execute"
                },
                "scheduler_mode": {
                    "type": "string",
                    "description": "[advance execute] forwarded to mission_plan execute"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "[advance execute] forwarded to mission_plan execute"
                }
            },
            "additionalProperties": true
        }),
    )]
}
