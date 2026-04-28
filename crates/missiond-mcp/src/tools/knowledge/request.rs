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
         request.lisp and surfaces request-local artifact paths + existence booleans. It never \
         auto-approves intent or plan, never bypasses mission_plan, and never spawns workstation \
         work directly. Wrapper response shape: { status, action, mode, request_artifacts, \
         projection: { status: written|skipped_*|write_failed, target?, sexp_source?, path?, \
         sha256?, bytes?, created?, overwritten?, error? }, pipeline, v3_contract, next_step, \
         review_packet?: { state: received|intent_drafting|awaiting_intent_approval|\
         awaiting_plan_approval|execute_requested, artifact_kind: request|intent_alignment|plan, \
         artifact_path, artifact_exists, artifact_preview (UTF-8-safe truncation, ≤480 bytes), \
         prompt, allowed_responses, next_action, execute_allowed } }. review_packet is a pure \
         projection of request-local artifact existence + the latest projection target — the \
         caller decides whether to approve via mission_directive/mission_plan; mission_request \
         never silently approves or dispatches.",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "advance", "status"],
                    "description": "start=create/request artifact + run next unified pipeline stage; advance=run next stage with approved_directive_id or approved_plan_id; status=read request.lisp"
                },
                "message": {
                    "type": "string",
                    "description": "[start] user need / external request body"
                },
                "request_id": {
                    "type": "string",
                    "description": "[start|advance|status] stable request id. Omit on start to allocate req-<uuid-prefix>."
                },
                "mode": {
                    "type": "string",
                    "enum": ["human_interactive", "trusted_agent"],
                    "description": "[start] v3 entry mode. human_interactive keeps both review gates; trusted_agent may fold intent into plan only through policy gates. Default human_interactive."
                },
                "project": {
                    "type": "string",
                    "description": "[start|status] registered project id used to resolve project root for .missiond/requests writes"
                },
                "cwd": {
                    "type": "string",
                    "description": "[start|status] absolute cwd inside a registered project; used when project is omitted"
                },
                "target_project": {
                    "type": "string",
                    "description": "[start|status] fallback project id used when project/cwd are omitted; also forwarded to mission_plan"
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
                    "description": "[advance] approved directive UUID; triggers plan-authoring path"
                },
                "directive_version": {
                    "type": "integer",
                    "description": "[advance] directive version forwarded to mission_plan compile"
                },
                "board_task_id": {
                    "type": "string",
                    "description": "[advance plan-authoring] board task anchor required by mission_plan compile"
                },
                "approved_plan_id": {
                    "type": "string",
                    "description": "[advance execute] approved plan UUID"
                },
                "execute": {
                    "type": "boolean",
                    "description": "[advance execute] must be true with approved_plan_id; mission_request never auto-executes on id alone"
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
