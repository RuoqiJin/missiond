use crate::ToolDefinition;
use serde_json::{json, Map, Value};

fn prop(ty: &str, description: &str) -> Value {
    json!({"type": ty, "description": description})
}

fn prop_enum(ty: &str, description: &str, variants: &[&str]) -> Value {
    json!({
        "type": ty,
        "enum": variants,
        "description": description,
    })
}

fn prop_no_type(description: &str) -> Value {
    json!({"description": description})
}

fn build_properties() -> Value {
    let mut p: Map<String, Value> = Map::new();

    p.insert("action".into(), prop_enum(
        "string",
        "start=create/request artifact + run next unified pipeline stage; advance=run next stage with approved_directive_id or approved_plan_id; status=read request.lisp; respond=answer a review_packet (approve_intent | reject_intent | ask_question | approve_plan | reject_plan | execute_plan) — delegates approve/execute to mission_directive/mission_plan and records review events under .missiond/requests/<request_id>/events",
        &["start", "advance", "status", "respond"],
    ));

    let review_decisions = &[
        "approve_intent",
        "reject_intent",
        "ask_question",
        "approve_plan",
        "reject_plan",
        "execute_plan",
    ];

    p.insert("response".into(), prop_enum(
        "string",
        "[respond] review decision — approve_intent dispatches to mission_directive approve, creates a hidden BoardTask anchor if no board_task_id is supplied, and then unified_entry plan-authoring projects request-local plan.lisp; approve_plan dispatches to mission_plan approve and materializes request-local plan.lisp into a draft Plan row when no plan_id exists, reusing plan.lisp's BoardTask anchor when present and stamping :plan_id/:version/:board_task_id back into request-local plan.lisp (never execute); execute_plan requires execute=true and routes through mission_plan execute, resolving plan_id from explicit args, plan.lisp, or a prior approve_plan event; reject_intent/reject_plan/ask_question only append a request-local review event",
        review_decisions,
    ));
    p.insert(
        "decision".into(),
        prop_enum("string", "[respond] alias for `response`", review_decisions),
    );

    p.insert("note".into(), prop(
        "string",
        "[respond] optional human note recorded in the request-local review event (required in spirit for reject_*/ask_question routes)",
    ));
    p.insert(
        "message".into(),
        prop("string", "[start] user need / external request body"),
    );
    p.insert("request_id".into(), prop(
        "string",
        "[start|advance|status|respond] stable request id. Omit on start to allocate req-<uuid-prefix>.",
    ));
    p.insert("mode".into(), prop_enum(
        "string",
        "[start] v3 entry mode. human_interactive keeps both review gates; trusted_agent may fold intent into plan only through policy gates. Default human_interactive.",
        &["human_interactive", "trusted_agent"],
    ));
    p.insert("project".into(), prop(
        "string",
        "[start|advance|status|respond] registered project id used to resolve project root for request-local writes and forwarded to mission_plan execute when applicable",
    ));
    p.insert("cwd".into(), prop(
        "string",
        "[start|advance|status|respond] absolute cwd inside a registered project; forwarded to mission_plan compile/execute when applicable",
    ));
    p.insert("target_project".into(), prop(
        "string",
        "[start|advance|status|respond] fallback project id used when project/cwd are omitted; also forwarded to mission_plan and rendered into dry-run PLAN.lisp as :target-project",
    ));
    p.insert("write_request_file".into(), prop(
        "boolean",
        "[start] default true. When true, writes request.lisp and 000001.event.lisp. Set false for preview-only routing.",
    ));
    p.insert("overwrite_file".into(), prop(
        "boolean",
        "[start|advance|respond approve_intent] allow replacing an existing request.lisp / initial event AND any request-local intent-alignment.lisp / plan.lisp projection produced from the inner compile sexp. Default false.",
    ));
    p.insert("compiler_mode".into(), prop_enum(
        "string",
        "[start|advance|respond approve_intent] forwarded to mission_directive / mission_plan compile. Default dry_run on those surfaces.",
        &["dry_run", "sonnet"],
    ));
    p.insert("persist".into(), prop(
        "boolean",
        "[start|advance|respond approve_intent] forwarded to directive/plan compile. Default false on inner surfaces.",
    ));
    p.insert("approved_directive_id".into(), prop(
        "string",
        "[advance|respond approve_intent/reject_intent] approved directive UUID; on advance triggers plan-authoring path; on respond identifies the directive to approve/reject without bypassing mission_directive's gate",
    ));
    p.insert(
        "directive_id".into(),
        prop(
            "string",
            "[respond approve_intent/reject_intent] alias for approved_directive_id",
        ),
    );
    p.insert("directive_version".into(), prop(
        "integer",
        "[advance|respond approve_intent/reject_intent] directive version forwarded to mission_plan compile or mission_directive approve",
    ));
    p.insert("board_task_id".into(), prop(
        "string",
        "[advance plan-authoring|respond approve_intent] board task anchor for mission_plan compile. respond approve_intent may omit this; MissionD creates a hidden request-local BoardTask anchor.",
    ));
    p.insert("approved_plan_id".into(), prop(
        "string",
        "[advance execute|respond approve_plan/reject_plan/execute_plan] approved plan UUID. respond approve_plan may omit this when request-local plan.lisp exists; MissionD materializes it before approval and writes the persisted ref back into plan.lisp.",
    ));
    p.insert(
        "plan_id".into(),
        prop(
            "string",
            "[respond approve_plan/reject_plan/execute_plan] alias for approved_plan_id",
        ),
    );
    p.insert("execute".into(), prop(
        "boolean",
        "[advance execute|respond execute_plan] must be true with approved_plan_id; mission_request never auto-executes on id alone. respond approve_plan ignores this flag — only execute_plan honours it",
    ));
    p.insert(
        "execute_after_approval".into(),
        prop("boolean", "Alias for execute"),
    );
    p.insert("topic".into(), prop(
        "string",
        "[start|advance] forwarded to file-first compatibility writers; defaults to request_id when mission_request allocates one",
    ));
    p.insert("write_file".into(), prop(
        "boolean",
        "[start|advance|respond approve_intent] forwarded to mission_directive / mission_plan file-first compatibility writer",
    ));
    p.insert("review_gate_policy".into(), prop_enum(
        "string",
        "[start|advance|respond approve_intent] forwarded to directive/plan compile review gate policy",
        &["manual", "emit_question", "off"],
    ));
    p.insert("emit_review_question".into(), prop(
        "boolean",
        "[start|advance|respond approve_intent] forwarded to directive/plan compile review gate",
    ));
    p.insert("review_question_text".into(), prop(
        "string",
        "[start|advance|respond approve_intent] forwarded to directive/plan compile review gate",
    ));
    p.insert("review_question_id".into(), prop(
        "string",
        "[start|advance|respond approve_intent/execute_plan] forwarded to directive/plan compile/execute surfaces",
    ));
    p.insert("dispatch_strategy".into(), prop(
        "string",
        "[advance|respond approve_intent/execute_plan] forwarded to mission_plan compile/execute; dry-run plan compile renders it as :dispatch-strategy",
    ));
    p.insert("parallelism".into(), prop(
        "string",
        "[advance|respond approve_intent/execute_plan] forwarded to mission_plan compile/execute; dry-run plan compile can derive :dispatch-strategy from it",
    ));
    p.insert("target".into(), prop_enum(
        "string",
        "[advance|respond approve_intent/execute_plan] forwarded to mission_plan compile/execute; dry-run plan compile renders it as :target, defaulting to mission_task_delegate when omitted",
        &["mission_execution", "mission_task_delegate", "mission_flow_run"],
    ));
    p.insert("objective".into(), prop(
        "string",
        "[advance|respond approve_intent/execute_plan] explicit plan objective. approve_intent forwards it into dry-run plan.lisp as :objective; execute_plan forwards it only as an override while the preferred path is deriving it from plan.lisp.",
    ));
    p.insert("requested_cwd".into(), prop(
        "string",
        "[advance|respond approve_intent/execute_plan] explicit execution cwd hint. approve_intent forwards it into dry-run plan.lisp as :requested-cwd; execute_plan forwards it only as an override while the preferred path is deriving it from plan.lisp.",
    ));
    p.insert("flow_id".into(), prop(
        "string",
        "[advance|respond approve_intent/execute_plan] explicit mission_flow_run id. approve_intent forwards it into plan compile; execute_plan forwards it only as an override while the preferred path is deriving it from plan.lisp :flow-id / :flow_id.",
    ));
    p.insert(
        "execute_mode".into(),
        prop_enum(
            "string",
            "[advance execute|respond execute_plan] forwarded to mission_plan execute",
            &["bridge", "internal"],
        ),
    );
    p.insert(
        "scheduler_mode".into(),
        prop(
            "string",
            "[advance execute|respond execute_plan] forwarded to mission_plan execute",
        ),
    );
    p.insert(
        "dry_run".into(),
        prop(
            "boolean",
            "[advance execute|respond execute_plan] forwarded to mission_plan execute",
        ),
    );
    p.insert("plan_acceptance".into(), prop_no_type(
        "[respond approve_intent] forwarded to mission_plan compile as acceptance context for request-local plan authoring",
    ));
    p.insert("plan_constraints".into(), prop_no_type(
        "[respond approve_intent] forwarded to mission_plan compile as constraint context for request-local plan authoring",
    ));

    Value::Object(p)
}

fn input_schema() -> Value {
    json!({
        "type": "object",
        "required": ["action"],
        "properties": build_properties(),
        "additionalProperties": true,
    })
}

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
         from the request-local intent-alignment.lisp / plan.lisp or prior request-local review \
         events; missing refs return a structured blocked response with next_action instead of fabricating \
         an id. approve_intent delegates to \
         mission_directive(action=approve), creates a hidden BoardTask anchor when no board_task_id \
         is supplied, then on successful approval immediately runs unified_entry \
         plan-authoring and projects request-local plan.lisp for the same request (dry_run plan-authoring \
         includes executable :target/:objective hints so execute_plan can derive routing from Lisp); approve_plan \
         delegates to mission_plan(action=approve), and when only request-local plan.lisp exists it first \
         materializes that Lisp into a draft Plan row, reusing plan.lisp's BoardTask anchor when present \
         or creating a hidden anchor only if needed, then stamps :plan_id/:version/:board_task_id back into \
         request-local plan.lisp so execute_plan can read the artifact directly; approve_plan never sets execute=true; execute_plan requires \
         execute=true (or response=execute_plan) and routes through the existing mission_plan execute \
         path via unified_entry. reject_intent / \
         reject_plan / ask_question never mutate directive/plan approval state and only append a \
         request-local review event under .missiond/requests/<request_id>/events/<seq>.event.lisp. \
         All actions never auto-approve intent or plan, never bypass mission_plan, and never spawn \
         workstation work directly. Wrapper response shape (start/advance): { status, action, mode, \
         request_artifacts, projection: { status: written|skipped_*|write_failed, target?, \
         sexp_source?, path?, sha256?, bytes?, created?, overwritten?, error? }, pipeline, v3_contract, \
         next_step, review_packet?: { state: received|intent_drafting|awaiting_intent_approval|\
         awaiting_plan_approval|awaiting_execution|execute_requested, artifact_kind: request|intent_alignment|plan, \
         artifact_path, artifact_exists, artifact_preview (UTF-8-safe truncation, ≤480 bytes), \
         prompt, allowed_responses, next_action, execute_allowed } }. respond response shape: \
         { status: ok|blocked, action: respond, mode, request_id, request_path, artifact_paths, \
         artifact_exists, respond_result: { decision, outcome: recorded|dispatched|blocked, \
         event_path, event_seq, event_sha256, event_bytes, execute, next_action, directive_id?, \
         directive_version?, plan_id?, inner_action?, blocked_reason?, note?, board_task_materialization?, \
         plan_materialization? }, review_packet, \
         next_action, v3_contract, projection?, board_task_materialization?, plan_materialization?, pipeline_result? }. review_packet is a pure projection of \
         request-local artifact existence + the latest projection target — the caller answers \
         review packets through mission_request(action=respond); mission_request never silently \
         approves or dispatches.",
        input_schema(),
    )]
}
