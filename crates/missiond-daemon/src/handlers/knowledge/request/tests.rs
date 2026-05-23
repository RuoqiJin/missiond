//! Regression tests for the mission_request facade.

use super::*;

#[test]
fn lisp_string_escapes_quotes_and_newlines() {
    assert_eq!(lisp_string("a\"b\nc"), "\"a\\\"b\\nc\"");
}

#[test]
fn request_lisp_carries_v3_policy() {
    let paths = request_paths_for(Path::new("/tmp/project"), "req-abc");
    let body = build_request_lisp(&RequestDoc {
        request_id: "req-abc",
        mode: RequestMode::TrustedAgent,
        source: "user_request",
        objective: "Ship it",
        created_at: "2026-04-28T00:00:00Z",
        paths: &paths,
    });
    assert!(body.contains(":schema \"missiond.request.v1\""));
    assert!(body.contains(":mode :trusted-agent"));
    assert!(body.contains(":trusted_agent_fast_path true"));
    assert!(body.contains(".missiond/v3/missiond-blueprint.lisp"));
}

#[test]
fn event_lisp_uses_v3_lifecycle_event_shape() {
    let body = build_event_lisp(
        "req-abc",
        "2026-04-28T00:00:00Z",
        "request_received",
        "Ship it",
    );
    assert!(body.contains("(lifecycle-event \"evt-req-abc-000001\""));
    assert!(body.contains(":schema \"missiond.lifecycle-event.v1\""));
    assert!(body.contains(":event_id \"evt-req-abc-000001\""));
    assert!(body.contains(":idempotency_key \"req-abc/request_received\""));
}

#[test]
fn request_paths_use_v3_layout() {
    let paths = request_paths_for(Path::new("/repo"), "req-1");
    assert_eq!(
        paths.request,
        Path::new("/repo").join(".missiond/requests/req-1/request.lisp")
    );
    assert_eq!(
        paths.initial_event,
        Path::new("/repo").join(".missiond/requests/req-1/events/000001.event.lisp")
    );
    assert_eq!(
        paths.intent_alignment,
        Path::new("/repo").join(".missiond/requests/req-1/intent-alignment.lisp")
    );
    assert_eq!(
        paths.plan,
        Path::new("/repo").join(".missiond/requests/req-1/plan.lisp")
    );
}

// ── projection helpers — pure, no AppState / no IO ──────────────────

#[test]
fn classify_projection_uses_pipeline_stage_first() {
    let meta = PipelineMeta {
        pipeline_stage: Some("s1_message_intake".into()),
        artifact_scope: Some("plan".into()),
    };
    // pipeline_stage wins over scope when both are present.
    assert_eq!(
        classify_projection_target(&meta),
        ProjectionTarget::IntentAlignment
    );

    let meta = PipelineMeta {
        pipeline_stage: Some("s4_plan_authoring".into()),
        artifact_scope: None,
    };
    assert_eq!(classify_projection_target(&meta), ProjectionTarget::Plan);

    let meta = PipelineMeta {
        pipeline_stage: Some("s6_execution_runner".into()),
        artifact_scope: None,
    };
    assert_eq!(classify_projection_target(&meta), ProjectionTarget::Execute);
}

#[test]
fn classify_projection_falls_back_to_scope() {
    let meta = PipelineMeta {
        pipeline_stage: None,
        artifact_scope: Some("directive".into()),
    };
    assert_eq!(
        classify_projection_target(&meta),
        ProjectionTarget::IntentAlignment
    );

    let meta = PipelineMeta {
        pipeline_stage: None,
        artifact_scope: Some("plan".into()),
    };
    assert_eq!(classify_projection_target(&meta), ProjectionTarget::Plan);

    let meta = PipelineMeta {
        pipeline_stage: None,
        artifact_scope: None,
    };
    assert_eq!(classify_projection_target(&meta), ProjectionTarget::Unknown);
}

#[test]
fn extract_sexp_prefers_compiled_over_preview() {
    let payload = json!({
        "compiled_sexp": "(directive :ok)",
        "compiled_sexp_preview": "(directive-draft)",
    });
    let (body, source) = extract_projected_sexp(&payload).expect("sexp present");
    assert_eq!(body, "(directive :ok)");
    assert_eq!(source, "compiled_sexp");
}

#[test]
fn extract_sexp_falls_back_to_preview() {
    let payload = json!({
        "compiled_sexp_preview": "(directive-draft)",
    });
    let (body, source) = extract_projected_sexp(&payload).expect("preview present");
    assert_eq!(body, "(directive-draft)");
    assert_eq!(source, "compiled_sexp_preview");
}

#[test]
fn extract_sexp_returns_none_when_blank_or_missing() {
    assert!(extract_projected_sexp(&json!({})).is_none());
    assert!(extract_projected_sexp(&json!({ "compiled_sexp": "" })).is_none());
    assert!(extract_projected_sexp(&json!({
        "compiled_sexp": null,
        "compiled_sexp_preview": ""
    }))
    .is_none());
}

#[test]
fn plan_projection_directive_preview_writes_intent_alignment() {
    let payload = json!({
        "status": "dry_run",
        "compiled_sexp_preview": "(directive-draft\n  :utterance \"do x\")\n",
    });
    let plan = plan_projection(ProjectionTarget::IntentAlignment, &payload, false);
    match plan {
        ProjectionPlan::Write {
            kind,
            body,
            sexp_source,
        } => {
            assert_eq!(kind, "intent_alignment");
            assert_eq!(sexp_source, "compiled_sexp_preview");
            assert!(body.contains("directive-draft"));
        }
        other => panic!("expected Write, got {:?}", other),
    }
}

#[test]
fn plan_projection_persisted_directive_preview_carries_ref() {
    let payload = json!({
        "status": "dry_run",
        "directive_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
        "compiled_sexp_preview": "(directive-draft\n  :utterance \"do x\"\n  :status :draft)\n",
    });
    let plan = plan_projection(ProjectionTarget::IntentAlignment, &payload, false);
    match plan {
        ProjectionPlan::Write { body, .. } => {
            assert!(body.contains(":directive_id \"00000000-0000-0000-0000-000000000abc\""));
            assert!(body.contains(":version 1"));
            assert_eq!(
                resolve_directive_ref(&json!({}), Some(&body))
                    .expect("projected ref resolves")
                    .id,
                "00000000-0000-0000-0000-000000000abc"
            );
        }
        other => panic!("expected Write, got {:?}", other),
    }
}

#[test]
fn plan_projection_plan_compile_writes_plan_body() {
    let payload = json!({
        "status": "compiled",
        "compiled_sexp": "(plan :board_task_id \"btk-1\")\n",
    });
    let plan = plan_projection(ProjectionTarget::Plan, &payload, false);
    match plan {
        ProjectionPlan::Write {
            kind,
            body,
            sexp_source,
        } => {
            assert_eq!(kind, "plan");
            assert_eq!(sexp_source, "compiled_sexp");
            assert!(body.contains("board_task_id"));
        }
        other => panic!("expected Write, got {:?}", other),
    }
}

#[test]
fn wrap_pipeline_result_exposes_request_local_artifacts_top_level() {
    let paths = request_paths_for(Path::new("/repo"), "req-wrap");
    let inner = ToolResult::json_pretty(&json!({
        "compiled_sexp_preview": "(directive-draft :status :draft)",
    }));
    let result = wrap_pipeline_result(
        "start",
        RequestMode::HumanInteractive,
        json!({ "request_id": "req-wrap" }),
        ProjectionOutcome::skipped(ProjectionStatus::SkippedNoSexp, Some("intent_alignment")),
        Some(&paths),
        false,
        inner,
    );
    let payload = tool_result_payload(&result);
    assert_eq!(
        payload["artifact_paths"]["intent_alignment"],
        "/repo/.missiond/requests/req-wrap/intent-alignment.lisp"
    );
    assert_eq!(payload["artifact_exists"]["intent_alignment"], false);
    assert_eq!(
        payload["review_packet"]["artifact_path"],
        payload["artifact_paths"]["intent_alignment"]
    );
}

#[test]
fn plan_projection_skips_on_pipeline_error() {
    let payload = json!({});
    let plan = plan_projection(ProjectionTarget::IntentAlignment, &payload, true);
    assert_eq!(
        plan,
        ProjectionPlan::Skip {
            status: ProjectionStatus::SkippedPipelineError,
            kind: None,
        }
    );
}

#[test]
fn plan_projection_skips_on_execute_target() {
    let payload = json!({ "compiled_sexp": "(execute)" });
    let plan = plan_projection(ProjectionTarget::Execute, &payload, false);
    assert_eq!(
        plan,
        ProjectionPlan::Skip {
            status: ProjectionStatus::SkippedExecuteStage,
            kind: None,
        }
    );
}

#[test]
fn plan_projection_skips_on_unknown_target() {
    let payload = json!({ "compiled_sexp": "(?)" });
    let plan = plan_projection(ProjectionTarget::Unknown, &payload, false);
    assert_eq!(
        plan,
        ProjectionPlan::Skip {
            status: ProjectionStatus::SkippedUnknownStage,
            kind: None,
        }
    );
}

#[test]
fn plan_projection_marks_no_sexp_when_payload_lacks_keys() {
    let payload = json!({ "status": "dry_run" });
    let plan = plan_projection(ProjectionTarget::Plan, &payload, false);
    assert_eq!(
        plan,
        ProjectionPlan::Skip {
            status: ProjectionStatus::SkippedNoSexp,
            kind: Some("plan"),
        }
    );
}

#[test]
fn projection_outcome_to_json_omits_unset_fields() {
    let outcome = ProjectionOutcome::skipped(ProjectionStatus::SkippedNoSexp, Some("plan"));
    let v = projection_to_json(&outcome);
    assert_eq!(v["status"], "skipped_no_sexp");
    assert_eq!(v["target"], "plan");
    assert!(v.get("path").is_none());
    assert!(v.get("sha256").is_none());
    assert!(v.get("error").is_none());
}

#[test]
fn build_artifact_paths_json_lists_all_six_keys() {
    let paths = request_paths_for(Path::new("/repo"), "req-x");
    let v = build_artifact_paths_json(&paths);
    for key in [
        "request",
        "intent_alignment",
        "plan",
        "events_dir",
        "receipts_dir",
        "reports_dir",
    ] {
        assert!(v.get(key).is_some(), "missing key {}", key);
        assert!(v[key].is_string(), "{} should be string", key);
    }
    assert!(v["intent_alignment"]
        .as_str()
        .unwrap()
        .ends_with("intent-alignment.lisp"));
    assert!(v["plan"].as_str().unwrap().ends_with("plan.lisp"));
}

#[test]
fn build_artifact_existence_with_predicate_drives_booleans() {
    let paths = request_paths_for(Path::new("/repo"), "req-x");
    // Pin only request.lisp + events_dir as existing; everything else absent.
    let exists = |p: &Path| p == paths.request.as_path() || p == paths.events_dir.as_path();
    let v = build_artifact_existence_with(&paths, exists);
    assert_eq!(v["request"], true);
    assert_eq!(v["events_dir"], true);
    assert_eq!(v["intent_alignment"], false);
    assert_eq!(v["plan"], false);
    assert_eq!(v["receipts_dir"], false);
    assert_eq!(v["reports_dir"], false);
}

// ── review_packet helpers — pure derivation, no AppState / no IO ───
fn paths_fixture() -> RequestPaths {
    request_paths_for(Path::new("/repo"), "req-rp")
}

fn no_read(_p: &Path) -> Option<String> {
    None
}

#[test]
fn classify_review_state_plan_present_wins_over_intent() {
    let existence = ArtifactExistence {
        request: true,
        intent_alignment: true,
        plan: true,
    };
    let (state, kind) = classify_review_state(existence, None, false, None);
    assert_eq!(state, ReviewState::AwaitingPlanApproval);
    assert_eq!(kind, "plan");
}

#[test]
fn classify_review_state_plan_approved_event_yields_awaiting_execution() {
    let existence = ArtifactExistence {
        request: true,
        intent_alignment: true,
        plan: true,
    };
    let (state, kind) = classify_review_state(
        existence,
        None,
        false,
        Some(ReviewEventCheckpoint::PlanApproved),
    );
    assert_eq!(state, ReviewState::AwaitingExecution);
    assert_eq!(kind, "plan");
}

#[test]
fn classify_review_state_plan_with_execute_yields_execute_requested() {
    let existence = ArtifactExistence {
        request: true,
        intent_alignment: false,
        plan: true,
    };
    let (state, kind) = classify_review_state(existence, None, true, None);
    assert_eq!(state, ReviewState::ExecuteRequested);
    assert_eq!(kind, "plan");
}

#[test]
fn classify_review_state_intent_only_yields_awaiting_intent() {
    let existence = ArtifactExistence {
        request: true,
        intent_alignment: true,
        plan: false,
    };
    let (state, kind) = classify_review_state(existence, None, false, None);
    assert_eq!(state, ReviewState::AwaitingIntentApproval);
    assert_eq!(kind, "intent_alignment");
}

#[test]
fn classify_review_state_no_artifacts_with_projection_target_drafts() {
    let existence = ArtifactExistence {
        request: true,
        intent_alignment: false,
        plan: false,
    };
    let (state, kind) = classify_review_state(existence, Some("plan"), false, None);
    assert_eq!(state, ReviewState::IntentDrafting);
    assert_eq!(kind, "plan");
}

#[test]
fn classify_review_state_default_is_received() {
    let existence = ArtifactExistence::default();
    let (state, kind) = classify_review_state(existence, None, false, None);
    assert_eq!(state, ReviewState::Received);
    assert_eq!(kind, "request");
}

#[test]
fn allowed_responses_match_blueprint_for_human_interactive() {
    assert_eq!(
        allowed_responses_for(
            RequestMode::HumanInteractive,
            ReviewState::AwaitingIntentApproval
        ),
        vec!["approve_intent", "reject_intent", "ask_question"]
    );
    assert_eq!(
        allowed_responses_for(
            RequestMode::HumanInteractive,
            ReviewState::AwaitingPlanApproval
        ),
        vec!["approve_plan", "reject_plan", "ask_question"]
    );
    assert_eq!(
        allowed_responses_for(
            RequestMode::HumanInteractive,
            ReviewState::AwaitingExecution
        ),
        vec!["execute_plan", "ask_question"]
    );
    assert_eq!(
        allowed_responses_for(RequestMode::HumanInteractive, ReviewState::Received),
        vec!["observe"]
    );
}

#[test]
fn allowed_responses_match_blueprint_for_trusted_agent() {
    assert_eq!(
        allowed_responses_for(
            RequestMode::TrustedAgent,
            ReviewState::AwaitingIntentApproval
        ),
        vec!["approve_intent", "ask_question"]
    );
    assert_eq!(
        allowed_responses_for(RequestMode::TrustedAgent, ReviewState::AwaitingPlanApproval),
        vec!["approve_plan", "ask_question"]
    );
    assert_eq!(
        allowed_responses_for(RequestMode::TrustedAgent, ReviewState::AwaitingExecution),
        vec!["execute_plan", "ask_question"]
    );
}

#[test]
fn build_review_artifact_preview_truncates_on_utf8_boundary() {
    // 60 Chinese characters * 3 bytes each = 180 bytes; ask for 80 bytes.
    let cjk: String = std::iter::repeat('好').take(60).collect();
    let preview = build_review_artifact_preview(Path::new("/x"), false, Some(&cjk), no_read, 80)
        .expect("preview");
    // Each '好' = 3 bytes. 80 / 3 = 26 chars * 3 = 78 bytes.
    assert_eq!(preview.len(), 78);
    assert_eq!(preview.chars().count(), 26);
    // Round-trip must remain valid UTF-8 (already a String, but pin the
    // intent: every byte boundary is a char boundary).
    for (i, _) in preview.char_indices() {
        assert!(preview.is_char_boundary(i));
    }
}

#[test]
fn build_review_artifact_preview_prefers_file_when_exists() {
    let read = |_p: &Path| Some("(plan :board_task_id \"btk-1\")\n".to_string());
    let preview =
        build_review_artifact_preview(Path::new("/x"), true, Some("(fallback)"), read, 480)
            .expect("preview");
    assert!(preview.contains("board_task_id"));
    assert!(!preview.contains("fallback"));
}

#[test]
fn build_review_artifact_preview_falls_back_when_file_absent() {
    let preview = build_review_artifact_preview(
        Path::new("/x"),
        false,
        Some("(directive-draft)"),
        no_read,
        480,
    )
    .expect("preview");
    assert_eq!(preview, "(directive-draft)");
}

#[test]
fn build_review_artifact_preview_returns_none_without_data() {
    let preview = build_review_artifact_preview(Path::new("/x"), false, None, no_read, 480);
    assert!(preview.is_none());
}

#[test]
fn derive_review_packet_intent_only_state() {
    let paths = paths_fixture();
    let inputs = ReviewPacketInputs {
        mode: RequestMode::HumanInteractive,
        paths: &paths,
        existence: ArtifactExistence {
            request: true,
            intent_alignment: true,
            plan: false,
        },
        projection_target: Some("intent_alignment"),
        fallback_preview: Some("(directive-draft)"),
        execute_requested: false,
        review_checkpoint: None,
    };
    let packet = derive_review_packet(&inputs, no_read);
    assert_eq!(packet["state"], "awaiting_intent_approval");
    assert_eq!(packet["artifact_kind"], "intent_alignment");
    assert_eq!(packet["artifact_exists"], true);
    assert_eq!(packet["execute_allowed"], false);
    assert_eq!(
        packet["next_action"],
        "call mission_request respond with response=approve_intent, reject_intent, or ask_question"
    );
    assert!(packet["artifact_path"]
        .as_str()
        .unwrap()
        .ends_with("intent-alignment.lisp"));
    let allowed: Vec<&str> = packet["allowed_responses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        allowed,
        vec!["approve_intent", "reject_intent", "ask_question"]
    );
}

#[test]
fn derive_review_packet_plan_present_overrides_intent() {
    let paths = paths_fixture();
    let inputs = ReviewPacketInputs {
        mode: RequestMode::HumanInteractive,
        paths: &paths,
        existence: ArtifactExistence {
            request: true,
            intent_alignment: true,
            plan: true,
        },
        projection_target: Some("plan"),
        fallback_preview: Some("(plan :ok)"),
        execute_requested: false,
        review_checkpoint: None,
    };
    let packet = derive_review_packet(&inputs, no_read);
    assert_eq!(packet["state"], "awaiting_plan_approval");
    assert_eq!(packet["artifact_kind"], "plan");
    assert_eq!(packet["execute_allowed"], false);
    assert_eq!(
        packet["next_action"],
        "call mission_request respond with response=approve_plan, reject_plan, or ask_question"
    );
}

#[test]
fn derive_review_packet_plan_approved_allows_execute_plan() {
    let paths = paths_fixture();
    let inputs = ReviewPacketInputs {
        mode: RequestMode::HumanInteractive,
        paths: &paths,
        existence: ArtifactExistence {
            request: true,
            intent_alignment: true,
            plan: true,
        },
        projection_target: None,
        fallback_preview: Some("(plan :ok)"),
        execute_requested: false,
        review_checkpoint: Some(ReviewEventCheckpoint::PlanApproved),
    };
    let packet = derive_review_packet(&inputs, no_read);
    assert_eq!(packet["state"], "awaiting_execution");
    assert_eq!(packet["execute_allowed"], true);
    assert_eq!(
        packet["next_action"],
        "call mission_request respond with response=execute_plan + execute=true"
    );
    let allowed: Vec<&str> = packet["allowed_responses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(allowed, vec!["execute_plan", "ask_question"]);
}

#[test]
fn latest_review_event_checkpoint_uses_latest_relevant_event() {
    let events = vec![
        "(lifecycle-event :payload (:decision :approve_plan :outcome :dispatched))".to_string(),
        "(lifecycle-event :payload (:decision :ask_question :outcome :recorded))".to_string(),
    ];
    assert_eq!(latest_review_event_checkpoint(&events), None);

    let events = vec![
        "(lifecycle-event :payload (:decision :approve_plan :outcome :dispatched))".to_string(),
    ];
    assert_eq!(
        latest_review_event_checkpoint(&events),
        Some(ReviewEventCheckpoint::PlanApproved)
    );

    let events = vec![
        "(lifecycle-event :payload (:decision :approve_plan :outcome :dispatched))".to_string(),
        "(lifecycle-event :payload (:decision :execute_plan :outcome :blocked))".to_string(),
    ];
    assert_eq!(
        latest_review_event_checkpoint(&events),
        Some(ReviewEventCheckpoint::PlanApproved)
    );
}

#[test]
fn derive_review_packet_execute_requested_when_plan_and_execute() {
    let paths = paths_fixture();
    let inputs = ReviewPacketInputs {
        mode: RequestMode::HumanInteractive,
        paths: &paths,
        existence: ArtifactExistence {
            request: true,
            intent_alignment: false,
            plan: true,
        },
        projection_target: None,
        fallback_preview: None,
        execute_requested: true,
        review_checkpoint: None,
    };
    let packet = derive_review_packet(&inputs, no_read);
    assert_eq!(packet["state"], "execute_requested");
    assert_eq!(packet["execute_allowed"], true);
    assert_eq!(
        packet["next_action"],
        "observe execution status through mission_request status and task receipts"
    );
    assert_eq!(packet["allowed_responses"][0], "observe");
}

#[test]
fn derive_review_packet_received_default_when_no_artifacts() {
    let paths = paths_fixture();
    let inputs = ReviewPacketInputs {
        mode: RequestMode::HumanInteractive,
        paths: &paths,
        existence: ArtifactExistence {
            request: true,
            intent_alignment: false,
            plan: false,
        },
        projection_target: None,
        fallback_preview: None,
        execute_requested: false,
        review_checkpoint: None,
    };
    let packet = derive_review_packet(&inputs, no_read);
    assert_eq!(packet["state"], "received");
    assert_eq!(packet["artifact_kind"], "request");
    assert_eq!(packet["execute_allowed"], false);
}

#[test]
fn derive_review_packet_intent_drafting_when_projection_targets_but_no_file() {
    let paths = paths_fixture();
    let inputs = ReviewPacketInputs {
        mode: RequestMode::HumanInteractive,
        paths: &paths,
        existence: ArtifactExistence::default(),
        projection_target: Some("intent_alignment"),
        fallback_preview: Some("(directive-draft)"),
        execute_requested: false,
        review_checkpoint: None,
    };
    let packet = derive_review_packet(&inputs, no_read);
    assert_eq!(packet["state"], "intent_drafting");
    assert_eq!(packet["artifact_kind"], "intent_alignment");
    assert_eq!(packet["artifact_exists"], false);
    assert_eq!(packet["artifact_preview"], "(directive-draft)");
}

#[test]
fn derive_review_packet_uses_safe_byte_truncation_for_cjk_preview() {
    let paths = paths_fixture();
    // ~120 bytes of CJK should be safely truncated to ≤80 bytes via
    // safe_byte_truncate. We feed it as the fallback preview to keep the
    // test pure (no file IO). Use a small max via the helper directly is
    // already covered above; here we just confirm derive_review_packet
    // does not panic on multi-byte input and produces a UTF-8 string.
    let cjk: String = std::iter::repeat('字').take(200).collect();
    let inputs = ReviewPacketInputs {
        mode: RequestMode::HumanInteractive,
        paths: &paths,
        existence: ArtifactExistence::default(),
        projection_target: Some("intent_alignment"),
        fallback_preview: Some(&cjk),
        execute_requested: false,
        review_checkpoint: None,
    };
    let packet = derive_review_packet(&inputs, no_read);
    let preview = packet["artifact_preview"]
        .as_str()
        .expect("preview present");
    assert!(preview.len() <= REVIEW_PREVIEW_MAX_BYTES);
    for (i, _) in preview.char_indices() {
        assert!(preview.is_char_boundary(i));
    }
}

#[test]
fn derive_review_packet_reads_artifact_file_via_callback() {
    let paths = paths_fixture();
    let inputs = ReviewPacketInputs {
        mode: RequestMode::HumanInteractive,
        paths: &paths,
        existence: ArtifactExistence {
            request: true,
            intent_alignment: false,
            plan: true,
        },
        projection_target: None,
        fallback_preview: None,
        execute_requested: false,
        review_checkpoint: None,
    };
    let read = |_p: &Path| Some("(plan :from-disk true)".to_string());
    let packet = derive_review_packet(&inputs, read);
    assert_eq!(packet["state"], "awaiting_plan_approval");
    let preview = packet["artifact_preview"].as_str().expect("preview");
    assert!(preview.contains("from-disk"));
}

#[test]
fn parse_execute_requested_handles_aliases() {
    assert!(!parse_execute_requested(&json!({})));
    assert!(parse_execute_requested(&json!({ "execute": true })));
    assert!(parse_execute_requested(
        &json!({ "execute_after_approval": true })
    ));
    assert!(!parse_execute_requested(&json!({ "execute": false })));
}

#[test]
fn extract_mode_from_request_lisp_recognizes_trusted_agent() {
    let trusted = "(mission-request foo\n  :mode :trusted-agent\n  :state :received)";
    assert_eq!(
        extract_mode_from_request_lisp(trusted),
        RequestMode::TrustedAgent
    );
    let human = "(mission-request foo\n  :mode :human-interactive)";
    assert_eq!(
        extract_mode_from_request_lisp(human),
        RequestMode::HumanInteractive
    );
    // Default safe-side: anything that isn't trusted-agent stays human-interactive.
    assert_eq!(
        extract_mode_from_request_lisp(""),
        RequestMode::HumanInteractive
    );
}

#[test]
fn extract_pipeline_meta_reads_decorator_sibling() {
    let inner_payload = json!({
        "status": "dry_run",
        "compiled_sexp_preview": "(directive-draft)",
    });
    let meta = json!({
        "pipeline_stage": "s1_message_intake",
        "artifact_refs": { "scope": "directive" },
    });
    let result = ToolResult {
        content: vec![
            ToolContent::Text {
                text: serde_json::to_string(&inner_payload).unwrap(),
            },
            ToolContent::Text {
                text: serde_json::to_string(&meta).unwrap(),
            },
        ],
        is_error: None,
    };
    let extracted = extract_pipeline_meta(&result);
    assert_eq!(
        extracted.pipeline_stage.as_deref(),
        Some("s1_message_intake")
    );
    assert_eq!(extracted.artifact_scope.as_deref(), Some("directive"));
}

// ── respond decision parsing — pure, no AppState ──────────────────

#[test]
fn parse_respond_decision_accepts_response_field() {
    let cases = [
        ("approve_intent", RespondDecision::ApproveIntent),
        ("reject_intent", RespondDecision::RejectIntent),
        ("ask_question", RespondDecision::AskQuestion),
        ("approve_plan", RespondDecision::ApprovePlan),
        ("reject_plan", RespondDecision::RejectPlan),
        ("execute_plan", RespondDecision::ExecutePlan),
    ];
    for (wire, expected) in cases {
        let parsed =
            parse_respond_decision(&json!({ "response": wire })).expect("decision should parse");
        assert_eq!(parsed, expected, "wire `{}`", wire);
        assert_eq!(parsed.wire(), wire);
    }
}

#[test]
fn parse_respond_decision_accepts_decision_alias() {
    let parsed = parse_respond_decision(&json!({ "decision": "approve_plan" }))
        .expect("decision should parse via alias");
    assert_eq!(parsed, RespondDecision::ApprovePlan);
}

#[test]
fn parse_respond_decision_response_wins_over_alias() {
    let parsed = parse_respond_decision(&json!({
        "response": "execute_plan",
        "decision": "approve_intent",
    }))
    .expect("decision should parse");
    assert_eq!(parsed, RespondDecision::ExecutePlan);
}

#[test]
fn parse_respond_decision_missing_returns_missing_param() {
    let err = parse_respond_decision(&json!({})).unwrap_err();
    assert_eq!(err, RespondParseError::Missing);
    let tool_err = err.into_tool_error();
    assert_eq!(tool_err.error_code, error_codes::MISSING_PARAM);
}

#[test]
fn parse_respond_decision_unknown_returns_invalid_param() {
    let err = parse_respond_decision(&json!({ "response": "approve_workflow" })).unwrap_err();
    assert!(matches!(err, RespondParseError::Unknown(_)));
    let tool_err = err.into_tool_error();
    assert_eq!(tool_err.error_code, error_codes::INVALID_PARAM);
}

#[test]
fn respond_decision_classification_matches_routing_table() {
    // approve_intent / reject_intent need a directive ref.
    for d in [
        RespondDecision::ApproveIntent,
        RespondDecision::RejectIntent,
    ] {
        assert!(d.requires_directive_ref());
        assert!(!d.requires_plan_ref());
    }
    // approve_plan / reject_plan / execute_plan need a plan ref.
    for d in [
        RespondDecision::ApprovePlan,
        RespondDecision::RejectPlan,
        RespondDecision::ExecutePlan,
    ] {
        assert!(!d.requires_directive_ref());
        assert!(d.requires_plan_ref());
    }
    // record-only routes — no directive/plan mutation, only event ledger.
    for d in [
        RespondDecision::RejectIntent,
        RespondDecision::RejectPlan,
        RespondDecision::AskQuestion,
    ] {
        assert!(d.record_only());
    }
    // approve_intent / approve_plan / execute_plan dispatch through the
    // existing inner surfaces.
    for d in [
        RespondDecision::ApproveIntent,
        RespondDecision::ApprovePlan,
        RespondDecision::ExecutePlan,
    ] {
        assert!(!d.record_only());
    }
}

// ── ref resolution — pure, no IO ──────────────────────────────────

#[test]
fn extract_lisp_keyword_string_finds_quoted_value() {
    let text = "(directive\n  :directive_id \"abc-123\"\n  :version 4)";
    assert_eq!(
        extract_lisp_keyword_string(text, "directive_id"),
        Some("abc-123".to_string())
    );
}

#[test]
fn extract_lisp_keyword_int_finds_numeric_value() {
    let text = "(directive\n  :directive_id \"abc-123\"\n  :version 4)";
    assert_eq!(extract_lisp_keyword_int(text, "version"), Some(4));
}

#[test]
fn extract_lisp_keyword_string_returns_none_when_missing() {
    let text = "(directive :goal :ship)";
    assert!(extract_lisp_keyword_string(text, "directive_id").is_none());
    assert!(extract_lisp_keyword_int(text, "version").is_none());
}

#[test]
fn extract_lisp_keyword_ignores_strings_and_comments() {
    let text = r#"(directive
      :note "debug :directive_id \"wrong\" :version 99"
      ; :directive_id "comment-wrong" :version 88
      :directive_id "right"
      :version 7)"#;
    assert_eq!(
        extract_lisp_keyword_string(text, "directive_id"),
        Some("right".to_string())
    );
    assert_eq!(extract_lisp_keyword_int(text, "version"), Some(7));
}

#[test]
fn extract_directive_ref_from_artifact_round_trip() {
    let text =
        "(directive :directive_id \"00000000-0000-0000-0000-000000000abc\" :directive_version 7)";
    let parsed = extract_directive_ref_from_artifact(text).expect("ref present");
    assert_eq!(parsed.id, "00000000-0000-0000-0000-000000000abc");
    assert_eq!(parsed.version, 7);
}

#[test]
fn resolve_directive_ref_prefers_explicit_args_over_artifact() {
    let args = json!({
        "approved_directive_id": "explicit-uuid",
        "directive_version": 9,
    });
    let artifact = "(directive :directive_id \"artifact-uuid\" :version 1)";
    let resolved = resolve_directive_ref(&args, Some(artifact)).expect("ref resolves");
    assert_eq!(resolved.id, "explicit-uuid");
    assert_eq!(resolved.version, 9);
}

#[test]
fn resolve_directive_ref_falls_back_to_artifact_when_args_missing() {
    let args = json!({});
    let artifact = "(directive :directive_id \"artifact-uuid\" :version 3)";
    let resolved = resolve_directive_ref(&args, Some(artifact)).expect("ref resolves");
    assert_eq!(resolved.id, "artifact-uuid");
    assert_eq!(resolved.version, 3);
}

#[test]
fn resolve_directive_ref_ignores_nested_non_uuid_id() {
    let artifact = r#"(intent-alignment
  :request_id "req-x"
  :scope (:id "root" :version 2)
  :version 2)"#;
    assert!(resolve_directive_ref(&json!({}), Some(artifact)).is_none());
}

#[test]
fn resolve_directive_ref_accepts_uuid_generic_id_for_legacy_artifacts() {
    let artifact = "(intent-alignment :id \"00000000-0000-0000-0000-000000000abc\" :version 2)";
    let resolved = resolve_directive_ref(&json!({}), Some(artifact)).expect("ref resolves");
    assert_eq!(resolved.id, "00000000-0000-0000-0000-000000000abc");
    assert_eq!(resolved.version, 2);
}

#[test]
fn resolve_directive_ref_returns_none_without_id_or_version() {
    let args = json!({});
    // Artifact lacks :directive_id / :version → blocked.
    let artifact = "(directive :goal :ship)";
    assert!(resolve_directive_ref(&args, Some(artifact)).is_none());
    // Args carry id but no version → still blocked (mission_directive
    // approve requires both).
    let args = json!({ "approved_directive_id": "x" });
    assert!(resolve_directive_ref(&args, None).is_none());
}

#[test]
fn resolve_plan_ref_prefers_args_then_artifact_then_blocks() {
    // Explicit arg wins.
    let args = json!({ "approved_plan_id": "explicit-plan" });
    let resolved =
        resolve_plan_ref(&args, Some("(plan :plan_id \"artifact-plan\")"), &[]).expect("plan ref");
    assert_eq!(resolved.id, "explicit-plan");
    // Falls back to artifact when args missing.
    let resolved = resolve_plan_ref(&json!({}), Some("(plan :plan_id \"artifact-plan\")"), &[])
        .expect("plan ref");
    assert_eq!(resolved.id, "artifact-plan");
    // Blocked when both missing.
    assert!(resolve_plan_ref(&json!({}), Some("(plan :goal :ship)"), &[]).is_none());
    assert!(resolve_plan_ref(&json!({}), None, &[]).is_none());
}

#[test]
fn resolve_plan_ref_ignores_dry_run_node_id() {
    let artifact = r#"(plan-draft
  :target "mission_task_delegate"
  :nodes [(:id "root" :objective "ship")])"#;
    assert!(resolve_plan_ref(&json!({}), Some(artifact), &[]).is_none());
}

#[test]
fn resolve_plan_ref_falls_back_to_latest_review_event() {
    let events = vec![
        "(event :plan_id \"old-plan\")".to_string(),
        "(event :decision :approve_plan :plan_id \"new-plan\")".to_string(),
    ];
    let resolved =
        resolve_plan_ref(&json!({}), Some("(plan :goal :ship)"), &events).expect("event ref");
    assert_eq!(resolved.id, "new-plan");
}

#[test]
fn enrich_materialized_plan_lisp_adds_ref_before_final_paren() {
    let body = "(plan-draft\n  :target \"mission_task_delegate\"\n  :nodes [(:id \"root\")])\n";
    let enriched = enrich_materialized_plan_lisp(
        body,
        &PlanRef {
            id: "plan-123".into(),
        },
        4,
        "board-456",
    );
    assert!(enriched.contains(":plan_id \"plan-123\""));
    assert!(enriched.contains(":version 4"));
    assert!(enriched.contains(":board_task_id \"board-456\""));
    assert!(enriched.ends_with(")\n"));
}

#[test]
fn enrich_materialized_plan_lisp_preserves_existing_ref() {
    let body = "(plan-draft :plan_id \"existing\" :version 2 :board_task_id \"b\")";
    assert_eq!(
        enrich_materialized_plan_lisp(body, &PlanRef { id: "new".into() }, 3, "new-board",),
        body
    );
}

#[test]
fn plan_materialization_json_exposes_ref_and_anchor() {
    let m = PlanMaterialization {
        plan_ref: PlanRef { id: "p1".into() },
        board_task_id: "b1".into(),
        version: 2,
        sexp_hash: "abc".into(),
        board_task_created: true,
        artifact_projection: Some(PlanArtifactProjection {
            path: PathBuf::from("/tmp/plan.lisp"),
            sha256: "def".into(),
            bytes: 12,
            overwritten: true,
        }),
        artifact_projection_error: None,
    };
    let v = plan_materialization_to_json(&m);
    assert_eq!(v["plan_id"], "p1");
    assert_eq!(v["board_task_id"], "b1");
    assert_eq!(v["version"], 2);
    assert_eq!(v["board_task_created"], true);
    assert_eq!(v["artifact_projection"]["sha256"], "def");
}

#[test]
fn respond_plan_compile_args_default_board_task_to_request_id() {
    let directive = DirectiveRef {
        id: "00000000-0000-0000-0000-000000000abc".into(),
        version: 7,
    };
    let args = json!({
        "compiler_mode": "dry_run",
        "project": "missiond",
        "persist": false,
        "directive_version": 99,
    });
    let out = build_respond_plan_compile_args(&args, &directive, "req-123");

    assert_eq!(out["approved_directive_id"], directive.id);
    assert_eq!(out["directive_version"], 7);
    assert_eq!(out["board_task_id"], "req-123");
    assert_eq!(out["compiler_mode"], "dry_run");
    assert_eq!(out["project"], "missiond");
    assert_eq!(out["persist"], false);
}

#[test]
fn respond_plan_compile_args_use_explicit_board_task() {
    let directive = DirectiveRef {
        id: "00000000-0000-0000-0000-000000000abc".into(),
        version: 1,
    };
    let args = json!({
        "board_task_id": "btk-42",
        "target": "mission_task_delegate",
        "target_project": "missiond",
        "objective": "ship from request-local PLAN",
        "requested_cwd": "/Users/jinchen/Projects/missiond",
        "write_file": true,
        "overwrite_file": true,
        "review_gate_policy": "manual",
    });
    let out = build_respond_plan_compile_args(&args, &directive, "req-123");

    assert_eq!(out["board_task_id"], "btk-42");
    assert_eq!(out["target"], "mission_task_delegate");
    assert_eq!(out["target_project"], "missiond");
    assert_eq!(out["objective"], "ship from request-local PLAN");
    assert_eq!(out["requested_cwd"], "/Users/jinchen/Projects/missiond");
    // Legacy write_file=true is preserved as a compat alias and forwarded.
    assert_eq!(out["write_file"], true);
    assert_eq!(out["overwrite_file"], true);
    assert_eq!(out["review_gate_policy"], "manual");
}

#[test]
fn respond_plan_compile_args_strips_write_file_by_default() {
    let directive = DirectiveRef {
        id: "00000000-0000-0000-0000-000000000abc".into(),
        version: 1,
    };
    // Caller did not opt into compat writes; the inner plan compile must
    // not receive write_file=true, even if the caller never set it.
    let args = json!({
        "board_task_id": "btk-42",
        "target": "mission_task_delegate",
        "objective": "request-local default flow",
        "overwrite_file": true,
    });
    let out = build_respond_plan_compile_args(&args, &directive, "req-123");
    assert!(
        out.get("write_file").is_none(),
        "default flow must not forward write_file to mission_plan compile"
    );
    // overwrite_file is unrelated to compat and still forwarded.
    assert_eq!(out["overwrite_file"], true);
}

#[test]
fn respond_plan_compile_args_forwards_compat_write_file() {
    let directive = DirectiveRef {
        id: "00000000-0000-0000-0000-000000000abc".into(),
        version: 1,
    };
    // V3-preferred name compat_write_file=true must turn into
    // write_file=true on the inner plan compile call.
    let args = json!({
        "board_task_id": "btk-42",
        "compat_write_file": true,
    });
    let out = build_respond_plan_compile_args(&args, &directive, "req-123");
    assert_eq!(out["write_file"], true);
    assert!(
        out.get("compat_write_file").is_none(),
        "mission_plan compile does not understand compat_write_file"
    );
}

#[test]
fn respond_plan_compile_args_compat_write_file_false_does_not_inject() {
    let directive = DirectiveRef {
        id: "00000000-0000-0000-0000-000000000abc".into(),
        version: 1,
    };
    // Explicit false on both flags must NOT forward write_file=true.
    let args = json!({
        "board_task_id": "btk-42",
        "compat_write_file": false,
        "write_file": false,
    });
    let out = build_respond_plan_compile_args(&args, &directive, "req-123");
    assert!(out.get("write_file").is_none());
    assert!(out.get("compat_write_file").is_none());
}

#[test]
fn normalize_start_args_strips_default_write_file() {
    // Default caller (no compat opt-in) — write_file must not survive
    // into the forwarded pipeline args.
    let mut args = json!({
        "compiler_mode": "dry_run",
        "persist": true,
    });
    normalize_start_args(&mut args, "req-1");
    assert_eq!(args["action"], "start-forwarded");
    assert_eq!(args["topic"], "req-1");
    assert!(args.get("write_file").is_none());
}

#[test]
fn normalize_start_args_preserves_legacy_write_file_alias() {
    // Legacy callers passing write_file=true still get compat writes.
    let mut args = json!({
        "write_file": true,
    });
    normalize_start_args(&mut args, "req-2");
    assert_eq!(args["write_file"], true);
}

#[test]
fn normalize_start_args_compat_write_file_true_normalizes_to_write_file() {
    let mut args = json!({
        "compat_write_file": true,
    });
    normalize_start_args(&mut args, "req-3");
    assert_eq!(args["write_file"], true);
    // V3-preferred name is consumed by mission_request; the inner
    // pipeline only knows about write_file.
    assert!(args.get("compat_write_file").is_none());
}

#[test]
fn normalize_start_args_strips_explicit_false_compat_keys() {
    let mut args = json!({
        "compat_write_file": false,
        "write_file": false,
    });
    normalize_start_args(&mut args, "req-4");
    assert!(args.get("compat_write_file").is_none());
    assert!(args.get("write_file").is_none());
}

// ── event sequencing — pure ────────────────────────────────────────

#[test]
fn next_event_seq_starts_after_initial_request_received_event() {
    // Only the initial request_received event has landed.
    let names = vec!["000001.event.lisp".to_string()];
    assert_eq!(next_event_seq(&names), 2);
}

#[test]
fn next_event_seq_picks_max_plus_one() {
    let names = vec![
        "000001.event.lisp".to_string(),
        "000002.event.lisp".to_string(),
        "000007.event.lisp".to_string(),
        "stray.txt".to_string(),
    ];
    assert_eq!(next_event_seq(&names), 8);
}

#[test]
fn next_event_seq_ignores_unrelated_filenames() {
    let names = vec![
        "README.md".to_string(),
        "000003.event.lisp.bak".to_string(),
        "abc.event.lisp".to_string(),
    ];
    // None match the strict <digits>.event.lisp pattern.
    assert_eq!(next_event_seq(&names), 1);
}

#[test]
fn next_event_seq_with_no_existing_events_starts_at_one() {
    let names: Vec<String> = Vec::new();
    assert_eq!(next_event_seq(&names), 1);
}

#[test]
fn event_path_for_seq_zero_pads_to_six_digits() {
    let path = event_path_for_seq(Path::new("/repo/.missiond/requests/req-x/events"), 5);
    assert_eq!(
        path,
        Path::new("/repo/.missiond/requests/req-x/events/000005.event.lisp")
    );
}

// ── review event lisp — pure render ───────────────────────────────

#[test]
fn build_review_event_lisp_records_dispatched_approve_intent() {
    let directive = DirectiveRef {
        id: "00000000-0000-0000-0000-000000000abc".into(),
        version: 4,
    };
    let body = build_review_event_lisp(&ReviewEventArgs {
        request_id: "req-rp",
        seq: 2,
        decision: RespondDecision::ApproveIntent,
        outcome: RespondOutcome::Dispatched,
        note: Some("looks good"),
        directive_ref: Some(&directive),
        plan_ref: None,
        execute: false,
        inner_action: Some("mission_directive::approve"),
        blocked_reason: None,
        created_at: "2026-04-28T00:00:00Z",
    });
    assert!(body.contains(":kind :review_response_dispatched"));
    assert!(body.contains(":decision :approve_intent"));
    assert!(body.contains(":outcome :dispatched"));
    assert!(body.contains(":directive_id \"00000000-0000-0000-0000-000000000abc\""));
    assert!(body.contains(":directive_version 4"));
    assert!(body.contains(":note \"looks good\""));
    assert!(body.contains(":execute false"));
    assert!(body.contains(":inner_action \"mission_directive::approve\""));
    assert!(body.contains(":idempotency_key \"req-rp/review_response_dispatched/000002\""));
}

#[test]
fn build_review_event_lisp_records_blocked_missing_plan_ref() {
    let body = build_review_event_lisp(&ReviewEventArgs {
        request_id: "req-rp",
        seq: 3,
        decision: RespondDecision::ExecutePlan,
        outcome: RespondOutcome::Blocked,
        note: None,
        directive_ref: None,
        plan_ref: None,
        execute: false,
        inner_action: None,
        blocked_reason: Some("plan ref missing"),
        created_at: "2026-04-28T00:00:00Z",
    });
    assert!(body.contains(":kind :review_response_blocked"));
    assert!(body.contains(":decision :execute_plan"));
    assert!(body.contains(":outcome :blocked"));
    assert!(body.contains(":blocked_reason \"plan ref missing\""));
    // Refs absent — ensure we did not invent fields.
    assert!(!body.contains(":directive_id"));
    assert!(!body.contains(":plan_id"));
}

#[test]
fn build_review_event_lisp_record_only_reject_plan_no_inner_action() {
    let plan = PlanRef {
        id: "11111111-1111-1111-1111-111111111111".into(),
    };
    let body = build_review_event_lisp(&ReviewEventArgs {
        request_id: "req-rp",
        seq: 4,
        decision: RespondDecision::RejectPlan,
        outcome: RespondOutcome::Recorded,
        note: Some("wrong scope"),
        directive_ref: None,
        plan_ref: Some(&plan),
        execute: false,
        inner_action: None,
        blocked_reason: None,
        created_at: "2026-04-28T00:00:00Z",
    });
    assert!(body.contains(":kind :review_response_recorded"));
    assert!(body.contains(":decision :reject_plan"));
    assert!(body.contains(":outcome :recorded"));
    assert!(body.contains(":plan_id \"11111111-1111-1111-1111-111111111111\""));
    assert!(body.contains(":note \"wrong scope\""));
    // reject_plan must NOT mutate approval state — no inner_action stamp.
    assert!(!body.contains(":inner_action"));
}

// ── next_action vocabulary — pure ─────────────────────────────────

#[test]
fn next_action_dispatched_paths_describe_continuation() {
    assert!(
        next_action_for(RespondDecision::ApproveIntent, RespondOutcome::Dispatched,)
            .contains("plan.lisp")
    );
    assert!(
        next_action_for(RespondDecision::ApprovePlan, RespondOutcome::Dispatched,)
            .contains("execute_plan")
    );
    assert!(
        next_action_for(RespondDecision::ExecutePlan, RespondOutcome::Dispatched,)
            .contains("execute")
    );
}

#[test]
fn next_action_blocked_message_describes_remediation() {
    let msg = next_action_for(RespondDecision::ApproveIntent, RespondOutcome::Blocked);
    assert!(msg.contains("missing"));
}

#[test]
fn next_action_record_only_paths_describe_followup() {
    assert!(
        next_action_for(RespondDecision::RejectIntent, RespondOutcome::Recorded,)
            .contains("revise")
    );
    assert!(
        next_action_for(RespondDecision::AskQuestion, RespondOutcome::Recorded,)
            .contains("question")
    );
}

// ── parse_event_seq_from_filename strictness ──────────────────────

#[test]
fn parse_event_seq_only_accepts_numeric_stem() {
    assert_eq!(parse_event_seq_from_filename("000001.event.lisp"), Some(1));
    assert_eq!(
        parse_event_seq_from_filename("999999.event.lisp"),
        Some(999999)
    );
    assert!(parse_event_seq_from_filename("abc.event.lisp").is_none());
    assert!(parse_event_seq_from_filename("000001.event.lisp.bak").is_none());
    assert!(parse_event_seq_from_filename("000001.lisp").is_none());
    assert!(parse_event_seq_from_filename("").is_none());
}
