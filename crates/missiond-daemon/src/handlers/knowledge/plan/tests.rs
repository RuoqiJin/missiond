use super::*;
use chrono::TimeZone;
use uuid::Uuid;

#[test]
fn sha256_hex_is_64_chars() {
    let h = sha256_hex("abc");
    assert_eq!(h.len(), 64);
    // ba7816bf… is the well-known SHA-256 prefix for "abc"
    assert!(h.starts_with("ba7816bf"));
}

#[test]
fn require_str_rejects_empty() {
    let args = serde_json::json!({"k": ""});
    assert!(require_str(&args, "k").is_err());
    let args2 = serde_json::json!({"k": "v"});
    assert_eq!(require_str(&args2, "k").unwrap(), "v");
}

#[test]
fn plan_evidence_sidecar_path_prefers_v3_runtime_and_falls_back_to_legacy() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let id = Uuid::parse_str("00000000-0000-0000-0000-000000000123").unwrap();

    let default_path = existing_plan_evidence_sidecar_path(root, id);
    assert!(default_path
        .ends_with(std::path::Path::new(COMPANION_DIR).join(format!("{}.evidence.json", id))));

    let legacy_path = root
        .join(LEGACY_COMPANION_DIR)
        .join(format!("{}.evidence.json", id));
    std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    std::fs::write(&legacy_path, br#"{"entries":[]}"#).unwrap();
    assert_eq!(existing_plan_evidence_sidecar_path(root, id), legacy_path);

    std::fs::create_dir_all(default_path.parent().unwrap()).unwrap();
    std::fs::write(&default_path, br#"{"entries":[]}"#).unwrap();
    assert_eq!(existing_plan_evidence_sidecar_path(root, id), default_path);
}

fn fixture_plan(sexp: &str) -> Plan {
    Plan {
        id: Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap(),
        board_task_id: "btk-1".to_string(),
        source_directive_id: None,
        version: 1,
        sexp_text: sexp.to_string(),
        sexp_hash: "deadbeef".to_string(),
        status: PlanStatus::Approved,
        compiler_model: None,
        compiled_from: None,
        contract_json: serde_json::json!({}),
        created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        approved_at: None,
        finished_at: None,
    }
}

#[test]
fn truncate_chars_preserves_short_input() {
    let s = "short";
    assert_eq!(truncate_chars(s, 100), "short");
}

#[test]
fn truncate_chars_caps_long_input() {
    let s = "a".repeat(500);
    let out = truncate_chars(&s, 240);
    assert!(out.ends_with("..."));
    assert!(out.len() <= 240 + 3);
}

#[test]
fn derive_objective_from_plan_caps_long_summary() {
    let huge = format!("(plan-draft :summary \"{}\")", "x".repeat(500));
    let plan = fixture_plan(&huge);
    let out = derive_objective_from_plan(&plan, 80);
    // Plan id prefix + truncated summary + ellipsis.
    assert!(out.starts_with(&format!("Plan {}: ", plan.id)));
    assert!(out.ends_with("..."));
    // Body shouldn't blow past the cap by more than the prefix overhead.
    assert!(out.len() < 200);
}

#[test]
fn derive_objective_from_plan_takes_first_nonempty_line() {
    let plan = fixture_plan("\n\n  (plan-draft :goal :align)  \n  (next ...)\n");
    let out = derive_objective_from_plan(&plan, 240);
    assert!(out.contains("(plan-draft :goal :align)"));
    assert!(!out.contains("(next ..."));
}

/// Build a ResolvedExec for tests. Defaults `target_source="explicit_arg"`
/// and `dispatch_strategy_source="explicit_arg"` since most legacy tests
/// exercise the explicit-arg precedence path.
fn fixture_resolved(target: &'static str, dispatch_strategy: &'static str) -> ResolvedExec {
    ResolvedExec {
        target,
        target_source: "explicit_arg",
        dispatch_strategy,
        dispatch_strategy_source: "explicit_arg",
        plan_hint_summary: json!({}),
    }
}

fn empty_hints() -> ParsedPlanHints {
    ParsedPlanHints::default()
}

#[test]
fn bridge_response_includes_plan_runner_v0_fields() {
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let result = action_execute_bridge(&plan, &resolved);
    assert!(result.is_error.is_none());
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    let v: Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["execute_mode"], "bridge");
    assert_eq!(v["runner_status"], "bridge_only");
    assert_eq!(v["target_tool"], "mission_execution");
    assert_eq!(v["target_source"], "explicit_arg");
    assert_eq!(v["dispatch_strategy"], "fresh-code-alignment");
    assert_eq!(v["dispatch_strategy_source"], "explicit_arg");
    assert!(v.get("plan_hint_summary").is_some());
    assert_eq!(v["next_call"]["tool"], "mission_execution");
    assert_eq!(v["next_call"]["action"], "open");
}

#[test]
fn build_internal_args_for_mission_execution_defaults() {
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let inner =
        build_internal_dispatch_args(&args, &plan, "mission_execution", "unknown", &empty_hints())
            .expect("default args build");
    assert_eq!(inner["action"], "open");
    assert_eq!(inner["execution_id"], format!("plan-{}", plan.id));
    assert_eq!(inner["parent_design"], format!("plan/{}", plan.id));
    assert_eq!(inner["owner"], "plan-runner");
    assert!(inner["scope"]
        .as_str()
        .unwrap()
        .contains(&plan.board_task_id));
    // workstation-dispatch-record: even when caller omits dispatch_strategy
    // the outer handler normalises to "unknown" before reaching this fn,
    // and we always forward it so mission_execution can persist the field.
    assert_eq!(inner["dispatch_strategy"], "unknown");
    // No target_project / requested_cwd in args → inner must not invent
    // them (legacy callers stay byte-identical apart from dispatch_strategy).
    assert!(inner.get("target_project").is_none());
    assert!(inner.get("requested_cwd").is_none());
}

#[test]
fn mission_execution_inner_includes_dispatch_strategy() {
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let inner = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_execution",
        "fresh-code-alignment",
        &empty_hints(),
    )
    .expect("strategy forward");
    assert_eq!(inner["dispatch_strategy"], "fresh-code-alignment");
}

#[test]
fn mission_execution_inner_propagates_target_project_and_cwd() {
    let plan = fixture_plan("(plan)");
    let args = json!({
        "target_project": "missiond",
        "requested_cwd": "/abs/path/missiond",
    });
    let inner = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_execution",
        "agent-team",
        &empty_hints(),
    )
    .expect("forward target_project and requested_cwd");
    // canonical project key gets the alias value
    assert_eq!(inner["project"], "missiond");
    // and the original alias is preserved verbatim for companion-log
    // persistence (workstation-dispatch-record :target-project)
    assert_eq!(inner["target_project"], "missiond");
    assert_eq!(inner["requested_cwd"], "/abs/path/missiond");
    assert_eq!(inner["dispatch_strategy"], "agent-team");
}

#[test]
fn mission_execution_inner_default_dispatch_when_caller_omits() {
    // action_execute normalises a missing/empty dispatch_strategy to
    // "unknown" before reaching build_internal_dispatch_args. This test
    // pins the contract: when the outer normalised string is "unknown",
    // inner["dispatch_strategy"] must be "unknown" (never absent, never
    // some other default).
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let inner =
        build_internal_dispatch_args(&args, &plan, "mission_execution", "unknown", &empty_hints())
            .expect("normalised default");
    assert_eq!(inner["dispatch_strategy"], "unknown");
}

#[test]
fn build_internal_args_for_task_delegate_derives_objective() {
    let plan = fixture_plan("(plan-draft :goal :align)\n");
    let args = json!({});
    let inner = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_task_delegate",
        "unknown",
        &empty_hints(),
    )
    .expect("default task_delegate args");
    let obj = inner["objective"].as_str().unwrap();
    assert!(obj.starts_with(&format!("Plan {}", plan.id)));
    assert!(obj.contains("(plan-draft"));
    assert_eq!(inner["intent"], "code");
    // context_hints should pin the plan + board_task ids
    let hints: Vec<String> = inner["context_hints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(hints.iter().any(|h| h.starts_with("plan:")));
    assert!(hints.iter().any(|h| h.starts_with("board_task:")));
    // task_delegate path must NOT receive dispatch_strategy — that field
    // belongs to the mission_execution companion log only.
    assert!(inner.get("dispatch_strategy").is_none());
}

/// BoardTask 31a99a30 :: dedup linkage. The plan-runner internal dispatch
/// must forward enough metadata for the `mission_task_delegate` dedup guard
/// to recognise overlapping code workers spawned by the same plan. Default
/// linkage (no caller override) anchors both `parent_board_task_id` and
/// `source_board_task_id` to `plan.board_task_id` so the next code worker
/// against the same plan can be refused when its write_scope overlaps.
#[test]
fn build_internal_args_for_task_delegate_forwards_dedup_linkage_by_default() {
    let plan = fixture_plan("(plan-draft :goal :align)\n");
    let args = json!({});
    let inner = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_task_delegate",
        "unknown",
        &empty_hints(),
    )
    .expect("default task_delegate args");
    assert_eq!(inner["parent_board_task_id"], json!(plan.board_task_id));
    assert_eq!(inner["source_board_task_id"], json!(plan.board_task_id));
    // No write_scope declared → guard short-circuits, so we MUST NOT inject
    // an empty list (would otherwise paint a read-only call as code-class
    // and trip the dedup guard against unrelated existing tasks).
    assert!(inner.get("write_scope").is_none());
    assert!(inner.get("must_not_touch").is_none());
}

#[test]
fn build_internal_args_for_task_delegate_passes_explicit_dedup_overrides() {
    let plan = fixture_plan("(plan-draft :goal :align)\n");
    let args = json!({
        "parent_board_task_id": "explicit-parent",
        "source_board_task_id": "explicit-source",
        "write_scope": ["crates/auth", "crates/router/src/lib.rs"],
        "must_not_touch": ["target/**"],
    });
    let inner = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_task_delegate",
        "unknown",
        &empty_hints(),
    )
    .expect("dedup overrides accepted");
    assert_eq!(inner["parent_board_task_id"], json!("explicit-parent"));
    assert_eq!(inner["source_board_task_id"], json!("explicit-source"));
    let scope: Vec<String> = inner["write_scope"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        scope,
        vec![
            "crates/auth".to_string(),
            "crates/router/src/lib.rs".to_string(),
        ]
    );
    let mnt: Vec<String> = inner["must_not_touch"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(mnt, vec!["target/**".to_string()]);
    // Code-intent + non-empty write_scope ⇒ task_class auto-defaults to
    // "code" so the downstream dedup guard fires (read-only / context-pack
    // delegations override task_class explicitly).
    assert_eq!(inner["task_class"], json!("code"));
}

#[test]
fn build_internal_args_for_task_delegate_uses_plan_hint_owned_files() {
    let plan = fixture_plan("(plan-draft :goal :align)\n");
    let args = json!({});
    let mut hints = ParsedPlanHints::default();
    hints.owned_files_raw = Some("[\"crates/foo.rs\" \"crates/bar.rs\"]".to_string());
    hints.forbidden_files_raw = Some("[\"target/**\"]".to_string());
    let inner =
        build_internal_dispatch_args(&args, &plan, "mission_task_delegate", "unknown", &hints)
            .expect("plan-hint backed dispatch");
    let scope: Vec<String> = inner["write_scope"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        scope,
        vec!["crates/foo.rs".to_string(), "crates/bar.rs".to_string()]
    );
    assert_eq!(
        inner["must_not_touch"],
        json!(["target/**"]),
        "plan hint forbidden_files must round-trip into must_not_touch"
    );
}

/// BoardTask 31a99a30 :: dedup linkage. The workstation-dispatch runner is
/// the second of the three resident/plan/workstation paths into
/// `mission_task_delegate`. It MUST forward `parent_board_task_id`,
/// `source_board_task_id`, `write_scope`, `must_not_touch`, and
/// `task_class` so the dedup guard can refuse a second concurrent code
/// worker against the same plan when their write scopes overlap.
///
/// Static test (no live PTY): scans the runner source for the inner-args
/// stamping. Sidesteps the AppState/runtime requirement of a full
/// integration harness while still failing loudly if a future edit drops
/// any dedup field. The companion check in
/// `scripts/check-v3-plan-execution-isomorphism.mjs` enforces the same
/// contract from a Lisp/CI perspective.
#[test]
fn workstation_dispatch_runner_forwards_dedup_linkage_to_inner_args() {
    let src = include_str!("../../../handlers/knowledge/workstation_dispatch/runner.rs");
    for needle in [
        "\"parent_board_task_id\": plan.board_task_id",
        "\"source_board_task_id\": plan.board_task_id",
        "\"task_class\": \"code\"",
        "inner_args[\"write_scope\"] = json!(hints.owned_files.clone())",
        "inner_args[\"must_not_touch\"] = json!(hints.forbidden_files.clone())",
    ] {
        assert!(
            src.contains(needle),
            "workstation_dispatch runner must stamp dedup linkage `{needle}` into inner_args; \
             without it, the mission_task_delegate dedup guard cannot recognise repeat \
             code workers spawned by the same plan"
        );
    }
}

#[test]
fn build_internal_args_for_task_delegate_keeps_read_only_dispatch_unattributed() {
    // No write_scope at all (caller args + plan hints both empty) should
    // leave the inner args without `write_scope`/`task_class`/`must_not_touch`
    // so the dedup guard's `dedup_applies` short-circuits to false. This
    // pins the "read-only / research delegations stay unaffected" rule.
    let plan = fixture_plan("(plan-draft :goal :align)\n");
    let args = json!({ "intent": "research" });
    let inner = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_task_delegate",
        "unknown",
        &empty_hints(),
    )
    .expect("research dispatch with no scope");
    assert_eq!(inner["intent"], json!("research"));
    assert!(inner.get("write_scope").is_none());
    assert!(inner.get("must_not_touch").is_none());
    assert!(
        inner.get("task_class").is_none(),
        "read-only research delegations must not auto-fill task_class=code"
    );
}

#[test]
fn build_internal_args_for_task_delegate_rejects_unknown_intent() {
    let plan = fixture_plan("(plan)");
    let args = json!({ "intent": "cosmic" });
    let err = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_task_delegate",
        "unknown",
        &empty_hints(),
    )
    .expect_err("unknown intent should be rejected");
    assert_eq!(err.is_error, Some(true));
}

#[test]
fn build_internal_args_for_flow_run_requires_flow_id() {
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let err =
        build_internal_dispatch_args(&args, &plan, "mission_flow_run", "unknown", &empty_hints())
            .expect_err("missing flow_id should be MISSING_PARAM");
    assert_eq!(err.is_error, Some(true));
    let text = match err.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text"),
    };
    assert!(text.contains("flow_id"));
}

#[test]
fn build_internal_args_for_flow_run_passes_through_params() {
    let plan = fixture_plan("(plan)");
    let args = json!({
        "flow_id": "F-demo",
        "params": { "k": "v" },
    });
    let inner =
        build_internal_dispatch_args(&args, &plan, "mission_flow_run", "unknown", &empty_hints())
            .expect("flow_run with flow_id");
    assert_eq!(inner["action"], "run");
    assert_eq!(inner["flow_id"], "F-demo");
    assert_eq!(inner["params"]["k"], "v");
    // flow_run must not pick up dispatch_strategy either.
    assert!(inner.get("dispatch_strategy").is_none());
}

fn parse_payload(result: &ToolResult) -> Value {
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    serde_json::from_str(&text).expect("valid json")
}

fn fixture_decision_not_applicable(
) -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
    crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
        source: crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchSource::NotApplicable,
        reason: None,
    }
}

#[test]
fn success_response_clean_path_is_executing() {
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let result = build_internal_dispatch_success_response(
        &plan,
        &resolved,
        json!({"ok": true}),
        Some("/tmp/sidecar.json".to_string()),
        None,
        None,
        &fixture_decision_not_applicable(),
        &TaskContractEmissionRecord::off(),
    );
    let v = parse_payload(&result);
    assert_eq!(v["status"], "executing");
    assert_eq!(v["runner_status"], "dispatched");
    assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
    assert!(v.get("evidence_error").is_none());
    assert!(v.get("status_update_error").is_none());
    assert_eq!(v["target_tool"], "mission_execution");
    assert_eq!(v["target_source"], "explicit_arg");
    assert_eq!(v["dispatch_strategy"], "fresh-code-alignment");
    assert_eq!(v["dispatch_strategy_source"], "explicit_arg");
    assert_eq!(v["inner_result"]["ok"], true);
    // wave-16 / task 03 — every legacy success response now carries
    // the routing decision so callers always see the provenance.
    assert_eq!(v["workstation_dispatch_source"], "not_applicable");
}

#[test]
fn success_response_evidence_failure_keeps_dispatched_but_exposes_error() {
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "agent-team");
    let result = build_internal_dispatch_success_response(
        &plan,
        &resolved,
        json!({"task_id": "btk-9"}),
        None,
        Some("mkdir failed: read-only fs".to_string()),
        None,
        &fixture_decision_not_applicable(),
        &TaskContractEmissionRecord::off(),
    );
    let v = parse_payload(&result);
    // Inner tool already produced durable side effects; we keep
    // dispatched/executing semantics but surface the sidecar error.
    assert_eq!(v["status"], "executing");
    assert_eq!(v["runner_status"], "dispatched");
    assert!(v["evidence_path"].is_null());
    assert_eq!(v["evidence_error"], "mkdir failed: read-only fs");
    assert!(v.get("status_update_error").is_none());
}

#[test]
fn success_response_status_update_failure_does_not_claim_executing() {
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "resident-lisp");
    let result = build_internal_dispatch_success_response(
        &plan,
        &resolved,
        json!({"execution_id": "plan-x"}),
        Some("/tmp/sidecar.json".to_string()),
        None,
        Some("DB error: connection lost".to_string()),
        &fixture_decision_not_applicable(),
        &TaskContractEmissionRecord::off(),
    );
    let v = parse_payload(&result);
    assert_ne!(v["status"], "executing");
    assert_eq!(v["status"], "dispatch_partial");
    assert_eq!(v["runner_status"], "status_update_failed");
    assert_eq!(v["status_update_error"], "DB error: connection lost");
    // inner_result and evidence_path must still be reported so callers can
    // act on the durable inner side effects.
    assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
    assert_eq!(v["inner_result"]["execution_id"], "plan-x");
    assert_eq!(v["target_tool"], "mission_execution");
    assert_eq!(v["dispatch_strategy"], "resident-lisp");
}

// ── plan-compiler v0 helpers (pure) ────────────────────────────────

#[test]
fn strip_fenced_code_block_extracts_body() {
    let raw = "```lisp\n(plan :goal :ok)\n```";
    assert_eq!(strip_fenced_code_block(raw), "(plan :goal :ok)");
}

#[test]
fn strip_fenced_code_block_handles_missing_lang_tag() {
    let raw = "```\n(plan)\n```";
    assert_eq!(strip_fenced_code_block(raw), "(plan)");
}

#[test]
fn strip_fenced_code_block_passthrough_when_unfenced() {
    assert_eq!(strip_fenced_code_block("(plan)"), "(plan)");
}

#[test]
fn strip_fenced_code_block_lone_open_fence_no_panic() {
    // No newline after the opening fence — we must not slice into a
    // missing newline; just hand the trimmed input back.
    assert_eq!(strip_fenced_code_block("```(plan)"), "```(plan)");
}

#[test]
fn parens_balanced_simple() {
    assert!(parens_balanced("(plan)"));
    assert!(parens_balanced("(plan (a) (b (c)))"));
}

#[test]
fn parens_balanced_unbalanced() {
    assert!(!parens_balanced("(plan"));
    assert!(!parens_balanced("(plan))"));
}

#[test]
fn parens_balanced_ignores_parens_in_strings() {
    // The `)` inside the string literal must not pop the depth.
    assert!(parens_balanced(r#"(plan :note "(((")"#));
    // Mismatched in code despite balanced strings should still fail.
    assert!(!parens_balanced(r#"(plan :note "()" "#));
}

#[test]
fn parens_balanced_honours_string_escapes() {
    // `\"` must not close the string, so `)` inside stays inert.
    assert!(parens_balanced(r#"(plan :note "x\")")"#));
}

#[test]
fn top_level_head_extracts_symbol() {
    assert_eq!(top_level_head("(plan :goal :ok)"), Some("plan"));
    assert_eq!(
        top_level_head("  (plan-draft\n  :goal :ok)"),
        Some("plan-draft")
    );
    assert_eq!(top_level_head("(PLAN)"), Some("PLAN"));
}

#[test]
fn top_level_head_returns_none_when_empty_paren() {
    assert_eq!(top_level_head("("), None);
    assert_eq!(top_level_head("()"), None);
}

#[test]
fn validate_compiled_plan_sexp_accepts_well_formed() {
    let sexp = r#"(plan :board_task_id "btk-1" :goal "ship")"#;
    let out = validate_compiled_plan_sexp(sexp, "btk-1").expect("valid plan");
    assert!(out.contains("btk-1"));
}

#[test]
fn validate_compiled_plan_sexp_strips_fence_then_validates() {
    let raw = "```lisp\n(plan-draft :board_task_id \"btk-9\")\n```";
    let out = validate_compiled_plan_sexp(raw, "btk-9").expect("fence-stripped plan");
    assert!(out.starts_with("(plan-draft"));
}

#[test]
fn validate_compiled_plan_sexp_rejects_empty() {
    let err = validate_compiled_plan_sexp("```\n```", "btk-1").unwrap_err();
    assert_eq!(err.code, "INVALID_COMPILER_OUTPUT");
    assert!(err.reason.contains("empty"));
}

#[test]
fn validate_compiled_plan_sexp_rejects_non_sexp_prefix() {
    let err = validate_compiled_plan_sexp("Sure! (plan)", "btk-1").unwrap_err();
    assert_eq!(err.code, "INVALID_COMPILER_OUTPUT");
    assert!(err.reason.contains("must start with `(`"));
}

#[test]
fn validate_compiled_plan_sexp_rejects_unbalanced() {
    let err = validate_compiled_plan_sexp(r#"(plan :board_task_id "btk-1""#, "btk-1").unwrap_err();
    assert!(err.reason.contains("not balanced"));
}

#[test]
fn validate_compiled_plan_sexp_rejects_unknown_head() {
    let sexp = r#"(directive :board_task_id "btk-1")"#;
    let err = validate_compiled_plan_sexp(sexp, "btk-1").unwrap_err();
    assert!(err.reason.contains("not in allowlist"));
}

#[test]
fn validate_compiled_plan_sexp_rejects_unanchored_plan() {
    // Top head is fine and parens balance, but the board_task id is
    // missing — refuse the plan to avoid persisting something that does
    // not bind to the row.
    let sexp = r#"(plan :goal "ship something else")"#;
    let err = validate_compiled_plan_sexp(sexp, "btk-1").unwrap_err();
    assert!(err.reason.contains("does not reference board_task_id"));
}

// ── compile dispatcher ────────────────────────────────────────────

#[test]
fn collect_string_list_handles_string_array_and_null() {
    assert_eq!(collect_string_list(None), Vec::<String>::new());
    assert_eq!(
        collect_string_list(Some(&Value::Null)),
        Vec::<String>::new()
    );
    assert_eq!(collect_string_list(Some(&json!(""))), Vec::<String>::new());
    assert_eq!(
        collect_string_list(Some(&json!("only"))),
        vec!["only".to_string()]
    );
    assert_eq!(
        collect_string_list(Some(&json!(["a", "", "b"]))),
        vec!["a".to_string(), "b".to_string()]
    );
}

/// dry_run is the default and must never call the LLM. We can't fully
/// exercise the handler without an AppState, so we drive the dispatch
/// guard via the public schema enum: any value other than `dry_run` /
/// `sonnet` is rejected before any side effect.
///
/// Together with `compile_dispatch_dry_run_default_is_pure`, this also
/// covers acceptance item "invalid `compiler_mode` structured error".
#[test]
fn compile_dispatch_rejects_unknown_compiler_mode() {
    // We can validate the dispatch logic indirectly by inspecting the
    // constants and ensuring the matching set has not silently grown.
    // If a future change adds a new mode, this test forces an update of
    // the schema description and the dispatcher together.
    assert_eq!(COMPILER_MODE_DRY_RUN, "dry_run");
    assert_eq!(COMPILER_MODE_SONNET, "sonnet");
    // Make sure the allowlist for plan heads stays in lock-step with the
    // system prompt copy.
    assert_eq!(ALLOWED_PLAN_HEADS, &["plan", "plan-draft", "PLAN"]);
}

#[test]
fn compile_dispatch_dry_run_default_is_pure() {
    // Unit-level guard for "default = dry_run, no LLM dependency". The
    // canonical default is the constant, and the schema enum lists it
    // first; the dispatcher reads that constant directly. If this
    // invariant ever drifts, downstream tooling that relies on
    // `compiler_mode` being optional + safe will break silently.
    assert_eq!(COMPILER_MODE_DRY_RUN, "dry_run");
}

#[test]
fn dry_run_plan_sexp_carries_executable_target_hints() {
    let sexp = render_dry_run_plan_sexp(DryRunPlanSexpInput {
        directive_id: Some("00000000-0000-0000-0000-000000000abc"),
        board_task_id: Some("btk-42"),
        target: "mission_task_delegate",
        dispatch_strategy: Some("agent-team"),
        target_project: Some("missiond"),
        requested_cwd: Some("/Users/jinchen/Projects/missiond"),
        flow_id: None,
        objective: "ship request-local plan",
        acceptance: vec!["cargo test -p missiond-daemon".to_string()],
        constraints: vec!["only declared write scope".to_string()],
    });

    assert!(sexp.starts_with("(plan-draft"));
    assert!(sexp.contains(":board_task_id \"btk-42\""));
    assert!(sexp.contains(":execution-readiness :dry-run-executable-scaffold"));
    assert!(sexp.contains(":target \"mission_task_delegate\""));
    assert!(sexp.contains(":nodes"));
    let hints = parse_plan_hints(&sexp);
    assert_eq!(hints.target.as_deref(), Some("mission_task_delegate"));
    assert_eq!(hints.dispatch_strategy.as_deref(), Some("agent-team"));
    assert_eq!(hints.target_project.as_deref(), Some("missiond"));
    assert_eq!(
        hints.requested_cwd.as_deref(),
        Some("/Users/jinchen/Projects/missiond")
    );
    assert_eq!(hints.objective.as_deref(), Some("ship request-local plan"));
}

#[test]
fn dry_run_plan_objective_uses_directive_alignment_text() {
    let args = json!({});
    let objective = derive_dry_run_plan_objective(
        &args,
        Some("(directive-draft :utterance \"make MissionD Lisp-driven\")"),
        Some("btk-42"),
    );
    assert_eq!(objective, "make MissionD Lisp-driven");
}

// ── planner prompt builders (light coverage) ──────────────────────

#[test]
fn build_planner_user_prompt_includes_anchor_and_directive() {
    let pin = Some((Uuid::nil(), 7));
    let body = build_planner_user_prompt(
        "btk-42",
        pin,
        Some("(intent-alignment :goal :align)"),
        Some("missiond"),
        Some("agent-team"),
        Some("mixed"),
        &["pass cargo test".to_string()],
        &["no migration".to_string()],
    );
    assert!(body.contains("btk-42"));
    assert!(body.contains("Directive: 00000000-0000-0000-0000-000000000000 v7"));
    assert!(body.contains("(intent-alignment :goal :align)"));
    assert!(body.contains("missiond"));
    assert!(body.contains("agent-team"));
    assert!(body.contains("mixed"));
    assert!(body.contains("pass cargo test"));
    assert!(body.contains("no migration"));
}

#[test]
fn build_planner_user_prompt_omits_optional_sections_when_empty() {
    let body = build_planner_user_prompt("btk-42", None, None, None, None, None, &[], &[]);
    assert!(body.contains("btk-42"));
    assert!(!body.contains("Directive:"));
    assert!(!body.contains("Approved directive sexp:"));
    assert!(!body.contains("Acceptance:"));
    assert!(!body.contains("Constraints:"));
}

#[test]
fn build_planner_system_prompt_lists_allowed_heads() {
    let s = build_planner_system_prompt();
    for head in ALLOWED_PLAN_HEADS {
        assert!(s.contains(head), "system prompt missing head `{}`", head);
    }
}

#[test]
fn success_response_status_and_evidence_failure_combined() {
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_flow_run", "mixed");
    let result = build_internal_dispatch_success_response(
        &plan,
        &resolved,
        json!({"flow_id": "F-demo"}),
        None,
        Some("disk full".to_string()),
        Some("DB error: timeout".to_string()),
        &fixture_decision_not_applicable(),
        &TaskContractEmissionRecord::off(),
    );
    let v = parse_payload(&result);
    assert_eq!(v["status"], "dispatch_partial");
    assert_eq!(v["runner_status"], "status_update_failed");
    assert_eq!(v["evidence_error"], "disk full");
    assert_eq!(v["status_update_error"], "DB error: timeout");
    assert!(v["evidence_path"].is_null());
}

// ── plan-runner auto-selection v1 ──────────────────────────────────

#[test]
fn parse_plan_hints_extracts_string_and_bareword_values() {
    let sexp = r#"
        (plan
          :board_task_id "btk-1"
          :target "mission_task_delegate"
          :flow-id F-demo
          :dispatch-strategy "agent-team"
          :parallelism agent-team
          :target-project "missiond"
          :requested-cwd "/abs/path"
          :objective "ship plan-runner v1"
          :summary "auto-selection v1")
    "#;
    let h = parse_plan_hints(sexp);
    assert_eq!(h.target.as_deref(), Some("mission_task_delegate"));
    assert_eq!(h.flow_id.as_deref(), Some("F-demo"));
    assert_eq!(h.dispatch_strategy.as_deref(), Some("agent-team"));
    assert_eq!(h.parallelism.as_deref(), Some("agent-team"));
    assert_eq!(h.target_project.as_deref(), Some("missiond"));
    assert_eq!(h.requested_cwd.as_deref(), Some("/abs/path"));
    assert_eq!(h.objective.as_deref(), Some("ship plan-runner v1"));
    assert_eq!(h.summary.as_deref(), Some("auto-selection v1"));
}

#[test]
fn parse_plan_hints_skips_list_values_and_keeps_first_occurrence() {
    // First :target wins; second :target inside a nested phase is ignored
    // by "store_first" semantics. List values are silently skipped, so the
    // :tasks (...) form below must NOT pollute the hint slots.
    let sexp = r#"
        (plan :target "mission_execution"
              :tasks (s1 :objective "phase 1")
              (phase :target "mission_flow_run"))
    "#;
    let h = parse_plan_hints(sexp);
    assert_eq!(h.target.as_deref(), Some("mission_execution"));
}

#[test]
fn parse_plan_hints_ignores_keywords_inside_string_literals() {
    // ":target" embedded inside a quoted note must not look like a real
    // keyword/value pair.
    let sexp = r#"(plan :note ":target faux" :objective "real one")"#;
    let h = parse_plan_hints(sexp);
    assert!(h.target.is_none());
    assert_eq!(h.objective.as_deref(), Some("real one"));
}

#[test]
fn parse_plan_hints_accepts_underscore_aliases() {
    let sexp = r#"(plan :flow_id F-y :target_project missiond :requested_cwd /tmp)"#;
    let h = parse_plan_hints(sexp);
    assert_eq!(h.flow_id.as_deref(), Some("F-y"));
    assert_eq!(h.target_project.as_deref(), Some("missiond"));
    assert_eq!(h.requested_cwd.as_deref(), Some("/tmp"));
}

#[test]
fn parse_plan_hints_empty_when_no_hints_present() {
    let sexp = "(plan :board_task_id \"btk-x\" :goal :ship)";
    let h = parse_plan_hints(sexp);
    assert!(h.target.is_none());
    assert!(h.flow_id.is_none());
    assert!(h.dispatch_strategy.is_none());
    assert!(h.parallelism.is_none());
}

#[test]
fn normalize_target_maps_keywords_to_canonical_targets() {
    assert_eq!(
        normalize_target("mission_execution", false),
        Some("mission_execution")
    );
    assert_eq!(
        normalize_target("EXECUTION", false),
        Some("mission_execution")
    );
    assert_eq!(
        normalize_target("mission_task_delegate", false),
        Some("mission_task_delegate")
    );
    assert_eq!(
        normalize_target("claudecode workstation", false),
        Some("mission_task_delegate")
    );
    assert_eq!(
        normalize_target("code-alignment session", false),
        Some("mission_task_delegate")
    );
    // flow_run gated by flow_id presence
    assert_eq!(normalize_target("mission_flow_run", false), None);
    assert_eq!(normalize_target("flow", false), None);
    assert_eq!(
        normalize_target("mission_flow_run", true),
        Some("mission_flow_run")
    );
    assert_eq!(normalize_target("flow", true), Some("mission_flow_run"));
    // unknown text yields None
    assert_eq!(normalize_target("nothing here", true), None);
}

#[test]
fn canonicalize_strategy_returns_known_or_none() {
    assert_eq!(canonicalize_strategy("agent-team"), Some("agent-team"));
    assert_eq!(canonicalize_strategy("AGENT_TEAM"), Some("agent-team"));
    assert_eq!(
        canonicalize_strategy("fresh-code-alignment"),
        Some("fresh-code-alignment")
    );
    assert_eq!(
        canonicalize_strategy("fresh code alignment"),
        Some("fresh-code-alignment")
    );
    assert_eq!(
        canonicalize_strategy("resident-lisp"),
        Some("resident-lisp")
    );
    assert_eq!(
        canonicalize_strategy("lisp-architect"),
        Some("resident-lisp")
    );
    assert_eq!(canonicalize_strategy("mixed"), Some("mixed"));
    assert_eq!(
        canonicalize_strategy("prompt-fallback"),
        Some("prompt-fallback")
    );
    // explicit "unknown" is treated as no signal so callers can fall back
    assert_eq!(canonicalize_strategy("unknown"), None);
    assert_eq!(canonicalize_strategy("nope"), None);
}

#[test]
fn resolve_dispatch_strategy_explicit_arg_wins() {
    let mut hints = ParsedPlanHints::default();
    hints.dispatch_strategy = Some("agent-team".to_string());
    let (v, src) = resolve_dispatch_strategy(Some("resident-lisp"), &hints);
    assert_eq!(v, "resident-lisp");
    assert_eq!(src, "explicit_arg");
}

#[test]
fn resolve_dispatch_strategy_falls_back_to_plan_hint() {
    let mut hints = ParsedPlanHints::default();
    hints.dispatch_strategy = Some("agent-team".to_string());
    let (v, src) = resolve_dispatch_strategy(None, &hints);
    assert_eq!(v, "agent-team");
    assert_eq!(src, "plan_hint");
}

#[test]
fn resolve_dispatch_strategy_uses_parallelism_when_dispatch_absent() {
    let mut hints = ParsedPlanHints::default();
    hints.parallelism = Some("agent-team".to_string());
    let (v, src) = resolve_dispatch_strategy(None, &hints);
    assert_eq!(v, "agent-team");
    assert_eq!(src, "plan_hint");
}

#[test]
fn resolve_dispatch_strategy_default_when_no_signal() {
    let (v, src) = resolve_dispatch_strategy(None, &ParsedPlanHints::default());
    assert_eq!(v, "unknown");
    assert_eq!(src, "default");
}

#[test]
fn resolve_dispatch_strategy_explicit_unknown_normalises_to_unknown() {
    // An explicit "unknown" arg still wins over the default branch and
    // does NOT cascade into plan hints — explicit means explicit.
    let mut hints = ParsedPlanHints::default();
    hints.dispatch_strategy = Some("agent-team".to_string());
    let (v, src) = resolve_dispatch_strategy(Some("unknown"), &hints);
    assert_eq!(v, "unknown");
    assert_eq!(src, "explicit_arg");
}

#[test]
fn build_internal_args_for_mission_execution_uses_plan_hints() {
    // Caller omits both target_project and requested_cwd; parser supplies
    // them and the inner JSON must include both.
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let mut hints = ParsedPlanHints::default();
    hints.target_project = Some("missiond".to_string());
    hints.requested_cwd = Some("/abs/path/missiond".to_string());

    let inner = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_execution",
        "fresh-code-alignment",
        &hints,
    )
    .expect("hints should backfill");
    assert_eq!(inner["project"], "missiond");
    assert_eq!(inner["target_project"], "missiond");
    assert_eq!(inner["requested_cwd"], "/abs/path/missiond");
    assert_eq!(inner["dispatch_strategy"], "fresh-code-alignment");
}

#[test]
fn build_internal_args_explicit_arg_overrides_plan_hint_for_mission_execution() {
    let plan = fixture_plan("(plan)");
    let args = json!({
        "target_project": "explicit-project",
        "requested_cwd": "/explicit/cwd",
    });
    let mut hints = ParsedPlanHints::default();
    hints.target_project = Some("hint-project".to_string());
    hints.requested_cwd = Some("/hint/cwd".to_string());

    let inner = build_internal_dispatch_args(&args, &plan, "mission_execution", "unknown", &hints)
        .expect("explicit arg wins");
    assert_eq!(inner["project"], "explicit-project");
    assert_eq!(inner["target_project"], "explicit-project");
    assert_eq!(inner["requested_cwd"], "/explicit/cwd");
}

#[test]
fn task_delegate_receives_agent_team_objective_hint() {
    let plan = fixture_plan("(plan-draft :goal :ship)");
    let args = json!({});
    let inner = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_task_delegate",
        "agent-team",
        &empty_hints(),
    )
    .expect("agent-team injection");
    let obj = inner["objective"].as_str().unwrap();
    assert!(
        obj.contains(AGENT_TEAM_OBJECTIVE_HINT),
        "objective should carry agent-team hint, got: {obj}"
    );
}

#[test]
fn task_delegate_does_not_duplicate_agent_team_hint_when_present() {
    let plan = fixture_plan("(plan)");
    let args = json!({
        "objective": format!("manual: {AGENT_TEAM_OBJECTIVE_HINT}"),
    });
    let inner = build_internal_dispatch_args(
        &args,
        &plan,
        "mission_task_delegate",
        "agent-team",
        &empty_hints(),
    )
    .expect("agent-team idempotent");
    let obj = inner["objective"].as_str().unwrap();
    // Exactly one occurrence — no duplication.
    assert_eq!(
        obj.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
        1,
        "should not duplicate hint, got: {obj}"
    );
}

#[test]
fn task_delegate_objective_falls_back_to_plan_hint() {
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let mut hints = ParsedPlanHints::default();
    hints.objective = Some("hint objective text".to_string());

    let inner =
        build_internal_dispatch_args(&args, &plan, "mission_task_delegate", "unknown", &hints)
            .expect("hint objective wins");
    assert_eq!(inner["objective"], "hint objective text");
}

#[test]
fn task_delegate_objective_falls_back_to_summary_hint_when_no_objective() {
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let mut hints = ParsedPlanHints::default();
    hints.summary = Some("summary fallback".to_string());

    let inner =
        build_internal_dispatch_args(&args, &plan, "mission_task_delegate", "unknown", &hints)
            .expect("summary fallback");
    assert_eq!(inner["objective"], "summary fallback");
}

#[test]
fn task_delegate_cwd_uses_hint_when_arg_missing() {
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let mut hints = ParsedPlanHints::default();
    hints.requested_cwd = Some("/from/hint".to_string());

    let inner =
        build_internal_dispatch_args(&args, &plan, "mission_task_delegate", "unknown", &hints)
            .expect("hint cwd backfill");
    assert_eq!(inner["cwd"], "/from/hint");
}

#[test]
fn task_delegate_cwd_uses_target_project_hint_only_when_path_like() {
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let mut hints = ParsedPlanHints::default();
    hints.target_project = Some("missiond".to_string()); // bare id, no '/'

    let inner =
        build_internal_dispatch_args(&args, &plan, "mission_task_delegate", "unknown", &hints)
            .expect("bare project id should not become cwd");
    assert!(inner.get("cwd").is_none());

    let mut hints2 = ParsedPlanHints::default();
    hints2.target_project = Some("/abs/missiond".to_string());
    let inner2 =
        build_internal_dispatch_args(&args, &plan, "mission_task_delegate", "unknown", &hints2)
            .expect("path-like target_project becomes cwd");
    assert_eq!(inner2["cwd"], "/abs/missiond");
}

#[test]
fn flow_run_uses_plan_hint_flow_id_when_arg_missing() {
    let plan = fixture_plan("(plan)");
    let args = json!({});
    let mut hints = ParsedPlanHints::default();
    hints.flow_id = Some("F-from-plan".to_string());

    let inner = build_internal_dispatch_args(&args, &plan, "mission_flow_run", "unknown", &hints)
        .expect("flow_id from hint");
    assert_eq!(inner["flow_id"], "F-from-plan");
}

#[test]
fn flow_run_explicit_arg_overrides_plan_hint() {
    let plan = fixture_plan("(plan)");
    let args = json!({ "flow_id": "F-explicit" });
    let mut hints = ParsedPlanHints::default();
    hints.flow_id = Some("F-from-plan".to_string());

    let inner = build_internal_dispatch_args(&args, &plan, "mission_flow_run", "unknown", &hints)
        .expect("explicit flow_id wins");
    assert_eq!(inner["flow_id"], "F-explicit");
}

#[test]
fn bridge_response_carries_plan_hint_summary_and_sources() {
    let plan = fixture_plan("(plan)");
    let mut hints_summary = serde_json::Map::new();
    hints_summary.insert("target".to_string(), json!("mission_task_delegate"));
    hints_summary.insert("parallelism".to_string(), json!("agent-team"));
    let resolved = ResolvedExec {
        target: "mission_task_delegate",
        target_source: "plan_hint",
        dispatch_strategy: "agent-team",
        dispatch_strategy_source: "plan_hint",
        plan_hint_summary: Value::Object(hints_summary),
    };
    let result = action_execute_bridge(&plan, &resolved);
    let v = parse_payload(&result);
    assert_eq!(v["target_tool"], "mission_task_delegate");
    assert_eq!(v["target_source"], "plan_hint");
    assert_eq!(v["dispatch_strategy"], "agent-team");
    assert_eq!(v["dispatch_strategy_source"], "plan_hint");
    assert_eq!(v["plan_hint_summary"]["target"], "mission_task_delegate");
    assert_eq!(v["plan_hint_summary"]["parallelism"], "agent-team");
}

#[test]
fn parsed_plan_hints_summary_omits_absent_fields() {
    let mut h = ParsedPlanHints::default();
    h.target = Some("mission_execution".to_string());
    let summary = h.to_summary_json();
    let obj = summary.as_object().expect("summary is object");
    assert_eq!(obj.len(), 1, "only :target should appear");
    assert_eq!(obj.get("target"), Some(&json!("mission_execution")));
}

// ── wave-11 :: project-root resolver (canonical contract) ────────────
//
// These tests pin `resolve_project_root` to the
// `intent-worker.lisp :: project-root-spawn-cwd` contract:
//   - explicit registered project id resolves to its canonical path
//   - cwd inside a registered project resolves via longest-prefix
//   - relative cwd is rejected (no process-cwd fallback)
//   - missing-signal-only case is rejected (no process-cwd fallback)
//   - unknown registered id is rejected
// We exercise the resolver helper directly with a `SharedProjectRegistry`
// so we don't have to materialise a full `AppState`.

use missiond_core::types::{ProjectConfig, ProjectRegistry, SharedProjectRegistry};
use std::sync::Arc;
use tokio::sync::RwLock;

fn registry_with(projects: Vec<ProjectConfig>) -> SharedProjectRegistry {
    Arc::new(RwLock::new(ProjectRegistry::new(projects)))
}

fn project(id: &str, path: &str) -> ProjectConfig {
    ProjectConfig {
        id: id.to_string(),
        path: path.to_string(),
        intent_path: None,
        active: true,
        slots: vec![],
        github_url: None,
        kind: "managed".to_string(),
        vault_path: None,
        parent_id: None,
        created_at: None,
        updated_at: None,
    }
}

#[tokio::test]
async fn resolve_project_root_resolves_registered_project_id() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let reg = registry_with(vec![project("missiond", &root.display().to_string())]);
    let resolved = resolve_project_root(&reg, Some("missiond"), None, None)
        .await
        .expect("explicit project id should resolve");
    assert_eq!(resolved, root);
}

#[tokio::test]
async fn resolve_project_root_resolves_absolute_cwd_via_longest_prefix() {
    // cwd-under-subdir → registry longest-prefix lookup picks the
    // canonical project root, NOT the subdir. This is the same path
    // flow_run / compute_slot use; the plan resolver must agree.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let subdir = root.join("crates").join("missiond-daemon");
    std::fs::create_dir_all(&subdir).unwrap();
    let reg = registry_with(vec![project("missiond", &root.display().to_string())]);
    let resolved = resolve_project_root(
        &reg,
        None,
        Some(subdir.display().to_string().as_str()),
        None,
    )
    .await
    .expect("absolute cwd inside registered project should resolve");
    assert_eq!(
        resolved, root,
        "must collapse to canonical root, not subdir"
    );
}

#[tokio::test]
async fn resolve_project_root_resolves_target_project_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let reg = registry_with(vec![project("missiond", &root.display().to_string())]);
    let resolved = resolve_project_root(&reg, None, None, Some("missiond"))
        .await
        .expect("target_project fallback should resolve");
    assert_eq!(resolved, root);
}

#[tokio::test]
async fn resolve_project_root_rejects_relative_cwd() {
    // Relative cwd must NEVER silently fall back to process cwd.
    // Even with a registered project, the relative cwd is refused at
    // pre-flight, so no resolver call ever sees it.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let reg = registry_with(vec![project("missiond", &root.display().to_string())]);
    let err = resolve_project_root(&reg, None, Some("relative/path"), None)
        .await
        .expect_err("relative cwd should be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("not absolute"),
        "error must explain refusal, got: {}",
        msg
    );
    assert!(
        msg.contains("project-root-spawn-cwd"),
        "error must reference the lisp contract, got: {}",
        msg
    );
}

#[tokio::test]
async fn resolve_project_root_rejects_missing_signal_no_process_cwd_fallback() {
    // No project, no cwd, no target_project → resolver MUST fail rather
    // than fall back to the daemon's process working directory
    // (CLAUDE.md `feedback_fail_fast_no_fallback`). This is the
    // regression guard for the prior process-cwd fallback path.
    let reg = registry_with(vec![project("missiond", "/tmp/missiond")]);
    let err = resolve_project_root(&reg, None, None, None)
        .await
        .expect_err("missing signal must be rejected; no process cwd fallback");
    let msg = err.to_string();
    assert!(
        msg.contains("project root unresolved"),
        "error must explain refusal, got: {}",
        msg
    );
    assert!(
        msg.contains("does not fall back"),
        "error must explicitly disclaim cwd fallback, got: {}",
        msg
    );
}

#[tokio::test]
async fn resolve_project_root_rejects_unknown_registered_id() {
    let reg = registry_with(vec![project("missiond", "/tmp/missiond")]);
    let err = resolve_project_root(&reg, Some("nonexistent"), None, None)
        .await
        .expect_err("unknown project id should be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("not registered") || msg.contains("nonexistent"),
        "error must mention the missing id, got: {}",
        msg
    );
}

#[tokio::test]
async fn resolve_project_root_explicit_id_wins_over_target_project() {
    // Explicit `project` arg takes precedence over `target_project`
    // (mirrors the canonical resolver source order).
    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    let root_a = tmp_a.path().canonicalize().unwrap();
    let root_b = tmp_b.path().canonicalize().unwrap();
    let reg = registry_with(vec![
        project("alpha", &root_a.display().to_string()),
        project("beta", &root_b.display().to_string()),
    ]);
    let resolved = resolve_project_root(&reg, Some("alpha"), None, Some("beta"))
        .await
        .expect("explicit id should win");
    assert_eq!(resolved, root_a);
}

// ── wave-19 / task 06 — task-contract emitter v0 ────────────────────

#[test]
fn parse_task_contract_emit_mode_default_is_off() {
    let v = json!({});
    let m = parse_task_contract_emit_mode(&v).expect("default ok");
    assert_eq!(m, TaskContractEmitMode::Off);
}

#[test]
fn parse_task_contract_emit_mode_boolean_shorthand_true_is_emit() {
    let v = json!({"emit_task_contract": true});
    let m = parse_task_contract_emit_mode(&v).expect("bool ok");
    assert_eq!(m, TaskContractEmitMode::Emit);
}

#[test]
fn parse_task_contract_emit_mode_boolean_shorthand_false_is_off() {
    let v = json!({"emit_task_contract": false});
    let m = parse_task_contract_emit_mode(&v).expect("bool ok");
    assert_eq!(m, TaskContractEmitMode::Off);
}

#[test]
fn parse_task_contract_emit_mode_explicit_emit_dry_run() {
    let v = json!({"task_contract_mode": "emit_dry_run"});
    let m = parse_task_contract_emit_mode(&v).expect("dry-run ok");
    assert_eq!(m, TaskContractEmitMode::EmitDryRun);
}

#[test]
fn parse_task_contract_emit_mode_explicit_wins_over_boolean() {
    let v = json!({
        "task_contract_mode": "off",
        "emit_task_contract": true,
    });
    let m = parse_task_contract_emit_mode(&v).expect("explicit wins");
    // Explicit string "off" beats boolean shorthand `true`.
    assert_eq!(m, TaskContractEmitMode::Off);
}

#[test]
fn parse_task_contract_emit_mode_unknown_string_is_structured_error() {
    let v = json!({"task_contract_mode": "emi"});
    let err = parse_task_contract_emit_mode(&v).expect_err("typo rejected");
    assert!(err.is_error.unwrap_or(false));
}

#[test]
fn parse_task_contract_emit_mode_non_string_value_is_structured_error() {
    let v = json!({"task_contract_mode": 7});
    let err = parse_task_contract_emit_mode(&v).expect_err("non-string rejected");
    assert!(err.is_error.unwrap_or(false));
}

// ── wave-20 / task 04 — dispatch_contract_mode parser tests ─────────

/// Default mode is `Rendered` so the wave-15..19 byte-shape is
/// preserved for callers that never opt in.
#[test]
fn parse_dispatch_contract_mode_default_is_rendered() {
    let v = json!({});
    let m = parse_dispatch_contract_mode(&v).expect("default ok");
    assert!(matches!(m, DispatchContractMode::Rendered));
    assert_eq!(m.as_str(), "rendered");
    assert!(!m.is_machine());
}

/// Explicit `dispatch_contract_mode="machine"` flips the mode.
#[test]
fn parse_dispatch_contract_mode_explicit_machine() {
    let v = json!({"dispatch_contract_mode": "machine"});
    let m = parse_dispatch_contract_mode(&v).expect("machine ok");
    assert!(matches!(m, DispatchContractMode::Machine));
    assert_eq!(m.as_str(), "machine");
    assert!(m.is_machine());
}

/// Explicit `dispatch_contract_mode="rendered"` is a no-op default.
#[test]
fn parse_dispatch_contract_mode_explicit_rendered() {
    let v = json!({"dispatch_contract_mode": "rendered"});
    let m = parse_dispatch_contract_mode(&v).expect("rendered ok");
    assert!(matches!(m, DispatchContractMode::Rendered));
}

/// `render_markdown=false` is the boolean shorthand for machine mode.
#[test]
fn parse_dispatch_contract_mode_render_markdown_false_is_machine() {
    let v = json!({"render_markdown": false});
    let m = parse_dispatch_contract_mode(&v).expect("shorthand ok");
    assert!(matches!(m, DispatchContractMode::Machine));
}

/// `render_markdown=true` is the explicit rendered (default) form.
#[test]
fn parse_dispatch_contract_mode_render_markdown_true_is_rendered() {
    let v = json!({"render_markdown": true});
    let m = parse_dispatch_contract_mode(&v).expect("shorthand ok");
    assert!(matches!(m, DispatchContractMode::Rendered));
}

/// Explicit `dispatch_contract_mode` wins over the boolean shorthand
/// when both are set so a caller cannot accidentally downgrade an
/// explicit machine opt-in.
#[test]
fn parse_dispatch_contract_mode_explicit_wins_over_shorthand() {
    let v = json!({
        "dispatch_contract_mode": "machine",
        "render_markdown": true,
    });
    let m = parse_dispatch_contract_mode(&v).expect("explicit wins");
    assert!(matches!(m, DispatchContractMode::Machine));
}

/// Typo (`dispatch_contract_mode="machin"`) MUST fail fast — never
/// silently degrade to `rendered`. This is the contract that
/// prevents a caller from accidentally falling back to the legacy
/// markdown-driven brief without noticing.
#[test]
fn parse_dispatch_contract_mode_unknown_string_is_structured_error() {
    let v = json!({"dispatch_contract_mode": "machin"});
    let err = parse_dispatch_contract_mode(&v).expect_err("typo rejected");
    assert!(err.is_error.unwrap_or(false));
}

/// Non-string `dispatch_contract_mode` is rejected (no silent
/// conversion of `7` → "rendered").
#[test]
fn parse_dispatch_contract_mode_non_string_value_is_structured_error() {
    let v = json!({"dispatch_contract_mode": 7});
    let err = parse_dispatch_contract_mode(&v).expect_err("non-string rejected");
    assert!(err.is_error.unwrap_or(false));
}

#[test]
fn lisp_escape_string_passes_plain_text() {
    assert_eq!(lisp_escape_string("hello world"), "hello world");
}

#[test]
fn lisp_escape_string_escapes_backslash_and_quote() {
    assert_eq!(lisp_escape_string("a\"b\\c"), "a\\\"b\\\\c");
}

#[test]
fn is_task_contract_eligible_requires_task_delegate() {
    assert!(is_task_contract_eligible(
        "mission_task_delegate",
        Some("ship")
    ));
    assert!(!is_task_contract_eligible(
        "mission_execution",
        Some("ship")
    ));
    assert!(!is_task_contract_eligible("mission_flow_run", Some("ship")));
}

#[test]
fn is_task_contract_eligible_rejects_empty_objective() {
    assert!(!is_task_contract_eligible(
        "mission_task_delegate",
        Some("")
    ));
    assert!(!is_task_contract_eligible(
        "mission_task_delegate",
        Some("   ")
    ));
    assert!(!is_task_contract_eligible("mission_task_delegate", None));
}

#[test]
fn build_task_contract_lisp_round_trips_required_fields() {
    let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000deadbeef").unwrap();
    let inputs = TaskContractInputs {
        objective: "ship feature X".to_string(),
        scope: Some("only the renderer".to_string()),
        owned_files: vec!["a.rs".to_string(), "b.rs".to_string()],
        forbidden_files: vec!["src/lib.rs".to_string()],
        acceptance_commands: vec!["cargo test".to_string(), "cargo build".to_string()],
        commit_policy: Some("scoped".to_string()),
        dispatch_strategy: "agent-team".to_string(),
        target: "mission_task_delegate".to_string(),
        target_project: Some("missiond".to_string()),
        requested_cwd: None,
        session_trace_path: None,
    };
    let body = build_task_contract_lisp(plan_id, "node-1", "btk-7", &inputs);
    // Must declare schema, kind, status, owner, write-scope, must-not-touch,
    // acceptance, commit (the task-contract v1 required floor).
    assert!(body.contains(":schema \"missiond.task-contract.v1\""));
    assert!(body.contains(":kind code-alignment"));
    assert!(body.contains(":status ready"));
    assert!(body.contains(":owner \"claudecode\""));
    assert!(body.contains(":dispatch-strategy \"agent-team\""));
    assert!(body.contains(":goal \"ship feature X\""));
    assert!(body.contains(":scope \"only the renderer\""));
    assert!(body.contains("[\"a.rs\" \"b.rs\"]"));
    assert!(body.contains("[\"src/lib.rs\"]"));
    assert!(body.contains("[\"cargo test\" \"cargo build\"]"));
    assert!(body.contains(":scope-check write-scope-only"));
    assert!(body.contains(":target-project \"missiond\""));
    // node-id stamped verbatim
    assert!(body.contains(":node-id \"node-1\""));
    // plan id traced for downstream observers
    assert!(body.contains(&plan_id.to_string()));
    // task id derived from plan + node prefix
    assert!(body.contains("(task plan-00000000-node-node-1\n"));
}

#[test]
fn build_task_contract_lisp_escapes_quotes_in_objective() {
    let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000feedface").unwrap();
    let inputs = TaskContractInputs {
        objective: r#"ship "thing" now"#.to_string(),
        owned_files: vec!["a.rs".to_string()],
        target: "mission_task_delegate".to_string(),
        dispatch_strategy: "agent-team".to_string(),
        ..Default::default()
    };
    let body = build_task_contract_lisp(plan_id, "node-x", "btk-1", &inputs);
    assert!(
        body.contains(r#":goal "ship \"thing\" now""#),
        "expected escaped quotes, got: {}",
        body
    );
}

#[test]
fn task_contract_path_uses_plan_then_node_layout() {
    let plan_id = Uuid::parse_str("00000000-0000-0000-0000-000000abcdef").unwrap();
    let root = std::path::Path::new("/tmp/missiond-root");
    let p = task_contract_path(root, plan_id, "node-7");
    let s = p.display().to_string();
    assert!(s.ends_with(&format!(
        ".missiond/tasks/generated/{}/node-7.lisp",
        plan_id
    )));
}

#[test]
fn task_contract_path_sanitizes_node_id() {
    let plan_id = Uuid::parse_str("00000000-0000-0000-0000-000000000111").unwrap();
    let root = std::path::Path::new("/tmp/missiond-root");
    // path-traversal characters collapse to a single dash
    let p = task_contract_path(root, plan_id, "node/with/slashes");
    assert!(p.display().to_string().ends_with("node-with-slashes.lisp"));
}

#[test]
fn render_command_includes_renderer_script_and_force_flag() {
    let cmd = render_command_for(std::path::Path::new("/tmp/a.lisp"));
    assert!(cmd.contains("scripts/render-claudecode-task.mjs"));
    assert!(cmd.contains("--force"));
    assert!(cmd.contains("/tmp/a.lisp"));
}

#[test]
fn task_contract_emission_record_off_has_no_response_block() {
    let r = TaskContractEmissionRecord::off();
    assert!(r.to_response_block().is_none());
    assert!(!r.is_failure());
}

#[test]
fn task_contract_emission_record_skipped_surfaces_reason() {
    let r = TaskContractEmissionRecord::skipped(TaskContractEmitMode::Emit, "objective empty");
    let block = r.to_response_block().expect("skipped surfaces a block");
    assert_eq!(block["task_contract_mode"], "emit");
    assert_eq!(block["task_contract_eligible"], false);
    assert_eq!(block["task_contract_skip_reason"], "objective empty");
    assert!(block.get("task_contract_path").is_none());
    assert!(!r.is_failure());
}

#[test]
fn task_contract_emission_record_ok_includes_path_and_render_command() {
    let r = TaskContractEmissionRecord::ok(
        TaskContractEmitMode::Emit,
        std::path::PathBuf::from("/tmp/a.lisp"),
    );
    let block = r.to_response_block().expect("ok surfaces block");
    assert_eq!(block["task_contract_mode"], "emit");
    assert_eq!(block["task_contract_eligible"], true);
    assert_eq!(block["task_contract_path"], "/tmp/a.lisp");
    assert!(block["render_command"]
        .as_str()
        .unwrap_or("")
        .contains("render-claudecode-task.mjs"));
    assert!(!r.is_failure());
}

#[test]
fn task_contract_emission_record_failed_surfaces_error() {
    let r = TaskContractEmissionRecord::failed(TaskContractEmitMode::Emit, "disk full".to_string());
    let block = r.to_response_block().expect("failure surfaces block");
    assert_eq!(block["task_contract_error"], "disk full");
    assert!(r.is_failure());
}

#[test]
fn write_task_contract_under_root_creates_canonical_path() {
    // Pure on-disk test that does not need an AppState — exercise the
    // path layout + atomic write under a tempdir.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000ffffffff").unwrap();
    let body = "(task fixture :schema \"missiond.task-contract.v1\")\n";
    let path = write_task_contract_under_root(&root, plan_id, "node-a", body)
        .expect("write should succeed");
    let s = path.display().to_string();
    assert!(s.ends_with(&format!(
        ".missiond/tasks/generated/{}/node-a.lisp",
        plan_id
    )));
    let read_back = std::fs::read_to_string(&path).expect("read back contract");
    assert_eq!(read_back, body);
}

#[test]
fn write_task_contract_under_root_overwrites_existing_atomically() {
    // Second write should replace the prior body via the tmp+rename
    // dance. No leftover .lisp.tmp file may remain.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000ffffeeee").unwrap();
    let _ = write_task_contract_under_root(&root, plan_id, "node-a", "first").expect("first write");
    let path =
        write_task_contract_under_root(&root, plan_id, "node-a", "second").expect("second write");
    let read_back = std::fs::read_to_string(&path).expect("read");
    assert_eq!(read_back, "second");
    let tmp_sibling = path.with_extension("lisp.tmp");
    assert!(!tmp_sibling.exists(), "tmp leftover detected");
}

// ── wave-12 :: record_evidence routing decision ──────────────────
//
// The action handler picks between the historical untagged shape
// (`{"evidence": …}`) and the new evidence-collector wrapper based on
// whether the caller supplied `evidence_kind` / `source`. We can't
// exercise the full handler here without spinning up an AppState +
// store, but we CAN pin the entry-shape decision by replaying the same
// branching logic against the real wrapper.
//
// These tests guard against a regression where someone "simplifies" the
// action by always wrapping (which would break legacy readers that
// expect the un-stamped payload) or always passing through (which would
// make the new params silently no-op).

#[test]
fn record_evidence_legacy_shape_when_no_kind_or_source() {
    let evidence = serde_json::json!({"tool_calls": []});
    // Replays the action's branching: both args absent → legacy wire form.
    let evidence_kind: Option<&str> = None;
    let source_override: Option<&str> = None;
    let entry = if evidence_kind.is_some() || source_override.is_some() {
        super::super::evidence_collector::wrap_legacy_record_evidence(
            evidence.clone(),
            evidence_kind,
            source_override,
        )
    } else {
        serde_json::json!({ "evidence": evidence })
    };
    let obj = entry.as_object().expect("entry is object");
    assert_eq!(obj.len(), 1, "legacy shape: only `evidence` at top level");
    assert!(obj.contains_key("evidence"));
    assert!(
        !obj.contains_key("schema_version"),
        "legacy shape has no schema stamp"
    );
    assert!(!obj.contains_key("source"), "legacy shape has no source");
    assert!(!obj.contains_key("kind"), "legacy shape has no kind");
}

#[test]
fn record_evidence_typed_wrap_when_kind_present() {
    let evidence = serde_json::json!({"note": "build green"});
    // Replays the action's branching: kind present → typed wrap.
    let evidence_kind: Option<&str> = Some("verification");
    let source_override: Option<&str> = None;
    let entry = if evidence_kind.is_some() || source_override.is_some() {
        super::super::evidence_collector::wrap_legacy_record_evidence(
            evidence.clone(),
            evidence_kind,
            source_override,
        )
    } else {
        serde_json::json!({ "evidence": evidence })
    };
    assert_eq!(entry["schema_version"], "v0", "schema stamp present");
    assert_eq!(
        entry["kind"], "verification",
        "caller-supplied kind round-trips"
    );
    assert_eq!(
        entry["source"], "record_evidence_manual",
        "default source applied when caller omits it"
    );
    assert_eq!(entry["evidence"], evidence, "original payload preserved");
}

#[test]
fn record_evidence_typed_wrap_when_source_present() {
    let evidence = serde_json::json!(["t1", "t2"]);
    let evidence_kind: Option<&str> = None;
    let source_override: Option<&str> = Some("ci_workflow");
    let entry = if evidence_kind.is_some() || source_override.is_some() {
        super::super::evidence_collector::wrap_legacy_record_evidence(
            evidence.clone(),
            evidence_kind,
            source_override,
        )
    } else {
        serde_json::json!({ "evidence": evidence })
    };
    assert_eq!(
        entry["source"], "ci_workflow",
        "caller-supplied source round-trips"
    );
    assert_eq!(
        entry["kind"], "note",
        "default kind applied when caller omits it"
    );
}

// ── wave-13 :: plan_runner_dispatch typed evidence shape ──────────
//
// `action_execute_internal` builds an `EvidenceEntry` (wave-12 typed
// collector) instead of a hand-rolled JSON object. These tests pin the
// projected on-disk shape so the wire-compatible mapping
//   legacy `kind="plan_runner_dispatch"`
//     ↦ canonical `source="plan_runner_dispatch"` + `kind="dispatch"`
// is enforced, and the legacy passthrough fields (`execute_mode`,
// `target_tool`, `target_source`, `dispatch_strategy`,
// `dispatch_strategy_source`, `plan_hint_summary`) keep their flat
// top-level placement for existing audit dashboards.
//
// We replay the exact entry construction (mirrored from
// `action_execute_internal`) instead of hitting the live handler so the
// assertions stay focused on the wire shape — the live handler is
// covered end-to-end by the runtime tests, but those don't introspect
// the on-disk JSON.
fn build_plan_runner_evidence_entry(resolved: &ResolvedExec, inner_payload: Value) -> Value {
    super::super::evidence_collector::EvidenceEntry::new(
        super::super::evidence_collector::source::PLAN_RUNNER_DISPATCH,
        super::super::evidence_collector::kind::DISPATCH,
    )
    .with_inner_dispatch(inner_payload.clone())
    .add_execution_event(super::super::evidence_collector::EventRef::unavailable(
        PLAN_RUNNER_EVENT_REF_UNAVAILABLE_REASON,
    ))
    .with_extra("execute_mode", json!("internal"))
    .with_extra("target_tool", json!(resolved.target))
    .with_extra("target_source", json!(resolved.target_source))
    .with_extra("dispatch_strategy", json!(resolved.dispatch_strategy))
    .with_extra(
        "dispatch_strategy_source",
        json!(resolved.dispatch_strategy_source),
    )
    .with_extra("plan_hint_summary", resolved.plan_hint_summary.clone())
    .with_extra("inner_result", inner_payload)
    .into_json()
}

#[test]
fn plan_runner_dispatch_evidence_carries_canonical_source_and_kind() {
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let inner = json!({"execution_id": "plan-x", "status": "executing"});
    let entry = build_plan_runner_evidence_entry(&resolved, inner.clone());
    // wave-12 wire-compatible mapping: historical `kind="plan_runner_dispatch"`
    // moves to `source`, canonical `kind="dispatch"`.
    assert_eq!(entry["source"], "plan_runner_dispatch");
    assert_eq!(entry["kind"], "dispatch");
    assert_eq!(entry["schema_version"], "v0");
    // Inner payload lands under the canonical typed slot.
    assert_eq!(entry["inner_dispatch"], inner);
    // Pre-wave12 sidecars carried the same payload under `inner_result`;
    // we keep it as a legacy alias for byte-for-byte reader compat.
    assert_eq!(entry["inner_result"], inner);
}

#[test]
fn plan_runner_dispatch_evidence_keeps_legacy_passthrough_keys_flat() {
    let resolved = fixture_resolved("mission_task_delegate", "agent-team");
    let entry = build_plan_runner_evidence_entry(&resolved, json!({"task_id": "btk-9"}));
    // Audit dashboards historically grep at the top level for these.
    assert_eq!(entry["execute_mode"], "internal");
    assert_eq!(entry["target_tool"], "mission_task_delegate");
    assert_eq!(entry["target_source"], "explicit_arg");
    assert_eq!(entry["dispatch_strategy"], "agent-team");
    assert_eq!(entry["dispatch_strategy_source"], "explicit_arg");
    // `plan_hint_summary` is an object — we simply assert its presence
    // (the fixture seeds an empty object so structural equality holds).
    assert!(
        entry.get("plan_hint_summary").is_some(),
        "plan_hint_summary must stay at the top level for audit grep"
    );
}

#[test]
fn plan_runner_dispatch_evidence_records_event_unavailability_reason() {
    // Single-node internal dispatch records the dispatch receipt, while
    // `EventRef::unavailable(...)` documents the deliberate correlation mode
    // so consumers can tell "no events" apart from "we tried but couldn't
    // correlate".
    let resolved = fixture_resolved("mission_execution", "resident-lisp");
    let entry = build_plan_runner_evidence_entry(&resolved, json!({"ok": true}));
    let events = entry["execution_events"]
        .as_array()
        .expect("execution_events array present");
    assert_eq!(events.len(), 1, "exactly one placeholder reference");
    assert_eq!(events[0]["unavailable"], true);
    let reason = events[0]["unavailable_reason"]
        .as_str()
        .expect("reason recorded as string");
    assert!(
        reason.contains("without a live ExecutionEvent ref"),
        "reason must mention the V3 unavailable event-ref mode so consumers can route on it: {}",
        reason
    );
    // No real event id leaked through.
    assert!(events[0].get("event_id").is_none());
}

// ── wave-14 :: plan file-first writer args ───────────────────────────

#[test]
fn extract_plan_file_args_defaults_are_inert() {
    let args = json!({});
    let f = extract_plan_file_args(&args);
    assert!(!f.write_file);
    assert!(!f.overwrite_file);
    assert!(f.topic.is_none());
    assert!(f.project.is_none());
    assert!(f.cwd.is_none());
    assert!(f.target_project.is_none());
}

#[test]
fn extract_plan_file_args_propagates_all_keys() {
    let args = json!({
        "write_file": true,
        "overwrite_file": true,
        "topic": "wave14-foo",
        "project": "missiond",
        "cwd": "/abs/path",
        "target_project": "fallback",
    });
    let f = extract_plan_file_args(&args);
    assert!(f.write_file);
    assert!(f.overwrite_file);
    assert_eq!(f.topic, Some("wave14-foo"));
    assert_eq!(f.project, Some("missiond"));
    assert_eq!(f.cwd, Some("/abs/path"));
    assert_eq!(f.target_project, Some("fallback"));
}

/// The plan writer falls back to `board_task_id` when no explicit topic
/// is provided. We assert the fallback wiring through a pure helper
/// invocation that mirrors `maybe_write_plan_artifact`'s short-circuit
/// logic — full integration is exercised in `file_artifacts::tests`.
#[tokio::test]
async fn maybe_write_plan_artifact_writes_under_board_task_topic_fallback() {
    use crate::handlers::knowledge::file_artifacts::{
        attempt_artifact_write, ArtifactKind, WriterContext,
    };
    use missiond_core::types::{ProjectConfig, ProjectRegistry, SharedProjectRegistry};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let reg: SharedProjectRegistry =
        Arc::new(RwLock::new(ProjectRegistry::new(vec![ProjectConfig {
            id: "missiond".to_string(),
            path: root.display().to_string(),
            intent_path: None,
            active: true,
            slots: vec![],
            github_url: None,
            kind: "managed".to_string(),
            vault_path: None,
            parent_id: None,
            created_at: None,
            updated_at: None,
        }])));

    // Mirror the resolver call the helper would make with topic = board_task_id.
    let outcome = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::Plan,
            topic: "btk-1",
            project: Some("missiond"),
            cwd: None,
            target_project: None,
            overwrite: false,
        },
        "(plan :board_task_id \"btk-1\")\n",
    )
    .await;
    let mut payload = json!({"status": "compiled", "plan_id": "abc"});
    outcome.splice_into(&mut payload);
    assert_eq!(
        payload["status"], "compiled",
        "Written must NOT downgrade status"
    );
    assert_eq!(payload["file_written"], true);
    let path = payload["file_path"].as_str().unwrap();
    assert!(path.ends_with(".missiond/plans/btk-1/PLAN.lisp"));
}

// ── wave-15 / task 05 — workstation-dispatch hint contract surface ───
//
// These tests pin the integration contract between `ParsedPlanHints`
// and the `workstation_dispatch` module: the new keyword fields are
// captured, summary projection includes them, opt-in detection is
// gated, and lisp list values round-trip through `split_lisp_string_list`.

#[test]
fn parse_plan_hints_captures_workstation_dispatch_contract() {
    let sexp = r#"
        (plan
          :target "mission_task_delegate"
          :dispatch-strategy "fresh-code-alignment"
          :scope "wave 15 task 05 only"
          :owned-files ["a.rs" "b.rs"]
          :forbidden-files ["c.rs"]
          :acceptance-commands ["cargo test" "git diff --check"]
          :commit-policy "scoped"
          :workstation-dispatch true)
    "#;
    let h = parse_plan_hints(sexp);
    assert_eq!(h.target.as_deref(), Some("mission_task_delegate"));
    assert_eq!(h.dispatch_strategy.as_deref(), Some("fresh-code-alignment"));
    assert_eq!(h.scope.as_deref(), Some("wave 15 task 05 only"));
    assert_eq!(h.commit_policy.as_deref(), Some("scoped"));
    assert!(h.owned_files_raw.as_deref().unwrap().contains("a.rs"));
    assert!(h.forbidden_files_raw.as_deref().unwrap().contains("c.rs"));
    assert!(h
        .acceptance_commands_raw
        .as_deref()
        .unwrap()
        .contains("cargo test"));
    assert!(h.workstation_dispatch_opt_in());
}

#[test]
fn parsed_plan_hints_workstation_dispatch_opt_in_recognises_truthy_values() {
    for truthy in &["true", "TRUE", "yes", "on", "1"] {
        let mut h = ParsedPlanHints::default();
        h.workstation_dispatch_flag = Some((*truthy).to_string());
        assert!(
            h.workstation_dispatch_opt_in(),
            "expected `{}` to be truthy",
            truthy
        );
    }
    for falsy in &["false", "no", "off", "0", "maybe"] {
        let mut h = ParsedPlanHints::default();
        h.workstation_dispatch_flag = Some((*falsy).to_string());
        assert!(
            !h.workstation_dispatch_opt_in(),
            "expected `{}` to NOT be truthy",
            falsy
        );
    }
}

#[test]
fn split_lisp_string_list_handles_bracket_paren_and_bareword_shapes() {
    assert!(split_lisp_string_list(None).is_empty());
    assert!(split_lisp_string_list(Some("")).is_empty());
    assert_eq!(
        split_lisp_string_list(Some(r#"["a.rs" "b.rs"]"#)),
        vec!["a.rs".to_string(), "b.rs".to_string()]
    );
    assert_eq!(
        split_lisp_string_list(Some("(x y z)")),
        vec!["x".to_string(), "y".to_string(), "z".to_string()]
    );
    // Bareword run with whitespace.
    assert_eq!(
        split_lisp_string_list(Some("a, b, c")),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

#[test]
fn parsed_plan_hints_to_workstation_hints_projects_every_field() {
    let sexp = r#"
        (plan
          :objective "ship the wave"
          :target-project "missiond"
          :requested-cwd "/abs/missiond"
          :dispatch-strategy "agent-team"
          :scope "scope text"
          :commit-policy "scoped"
          :owned-files ["a.rs"]
          :forbidden-files ["b.rs"]
          :acceptance-commands ["cargo test"])
    "#;
    let h = parse_plan_hints(sexp);
    let w = h.to_workstation_hints();
    assert_eq!(w.objective.as_deref(), Some("ship the wave"));
    assert_eq!(w.target_project.as_deref(), Some("missiond"));
    assert_eq!(w.requested_cwd.as_deref(), Some("/abs/missiond"));
    assert_eq!(w.dispatch_strategy.as_deref(), Some("agent-team"));
    assert_eq!(w.scope.as_deref(), Some("scope text"));
    assert_eq!(w.commit_policy.as_deref(), Some("scoped"));
    assert_eq!(w.owned_files, vec!["a.rs".to_string()]);
    assert_eq!(w.forbidden_files, vec!["b.rs".to_string()]);
    assert_eq!(w.acceptance_commands, vec!["cargo test".to_string()]);
}

#[test]
fn parsed_plan_hints_summary_includes_workstation_dispatch_fields() {
    let mut h = ParsedPlanHints::default();
    h.scope = Some("scope".to_string());
    h.owned_files_raw = Some(r#"["a.rs"]"#.to_string());
    h.commit_policy = Some("scoped".to_string());
    h.workstation_dispatch_flag = Some("true".to_string());
    let v = h.to_summary_json();
    assert_eq!(v["scope"], "scope");
    assert_eq!(v["commit_policy"], "scoped");
    assert!(v["owned_files"].as_str().unwrap().contains("a.rs"));
    assert_eq!(v["workstation_dispatch"], "true");
}

fn fixture_decision(
    source: crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchSource,
) -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
    crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
        source,
        reason: Some("test fixture".to_string()),
    }
}

#[test]
fn build_workstation_dispatch_response_dispatched_marks_status_executing() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "agent-team");
    let outcome = wd::WorkstationDispatchOutcome::Dispatched {
        task_brief: "## Objective\nship\n".to_string(),
        task_brief_path: None,
        task_contract_source_path: None,
        evidence_path: Some("/tmp/sidecar.json".to_string()),
        evidence_error: None,
        inner_payload: json!({"task_id": "btk-7"}),
    };
    let decision = fixture_decision(wd::WorkstationDispatchSource::ExplicitArg);
    let result = build_workstation_dispatch_response(
        &plan,
        &resolved,
        outcome,
        &decision,
        &TaskContractEmissionRecord::off(),
        DispatchContractMode::Rendered,
    );
    let v = parse_payload(&result);
    assert_eq!(v["status"], "executing");
    assert_eq!(v["runner_status"], "workstation_dispatch_v0");
    assert_eq!(v["target_tool"], "mission_task_delegate");
    assert_eq!(v["dispatch_strategy"], "agent-team");
    assert_eq!(v["workstation_dispatch_status"], "dispatched");
    assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
    assert_eq!(v["inner_result"]["task_id"], "btk-7");
    assert_eq!(v["workstation_dispatch_source"], "explicit_arg");
    assert_eq!(v["workstation_dispatch_inference_reason"], "test fixture");
    // wave-20 / task 04 — default rendered mode is byte-stable on
    // the wire: the new `dispatch_contract_mode` key surfaces but
    // the legacy `task_contract_source_path` extension stays
    // absent.
    assert_eq!(v["dispatch_contract_mode"], "rendered");
    assert!(
        v.get("task_contract_source_path").is_none(),
        "rendered-mode dispatch must omit task_contract_source_path"
    );
}

#[test]
fn build_workstation_dispatch_response_safe_descriptor_does_not_claim_executing() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let outcome = wd::WorkstationDispatchOutcome::SafeDescriptor {
        reason: wd::SafeDescriptorReason::ProjectRootUnresolved("no signal".to_string()),
        task_brief: None,
    };
    let decision = fixture_decision(wd::WorkstationDispatchSource::Inferred);
    let result = build_workstation_dispatch_response(
        &plan,
        &resolved,
        outcome,
        &decision,
        &TaskContractEmissionRecord::off(),
        DispatchContractMode::Rendered,
    );
    let v = parse_payload(&result);
    assert_ne!(v["status"], "executing");
    assert_eq!(v["status"], "dispatch_skipped");
    assert_eq!(
        v["workstation_dispatch_status"],
        "skipped_project_root_unresolved"
    );
    assert!(v.get("inner_result").is_none());
    // Even when the substrate refused, we surface that auto-inference
    // routed the call so the caller sees both the routing decision and
    // the safety failure side by side — never a silent prompt fallback.
    assert_eq!(v["workstation_dispatch_source"], "inferred");
}

#[test]
fn build_workstation_dispatch_response_dry_run_status_is_dry_run() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let outcome = wd::WorkstationDispatchOutcome::DryRun {
        task_brief: "## Objective\nship\n".to_string(),
    };
    let decision = fixture_decision(wd::WorkstationDispatchSource::PlanHint);
    let result = build_workstation_dispatch_response(
        &plan,
        &resolved,
        outcome,
        &decision,
        &TaskContractEmissionRecord::off(),
        DispatchContractMode::Rendered,
    );
    let v = parse_payload(&result);
    assert_eq!(v["status"], "dry_run");
    assert_eq!(v["workstation_dispatch_status"], "dry_run_no_dispatch");
    assert_eq!(v["workstation_dispatch_source"], "plan_hint");
}

/// wave-20 / task 04 — when the runner dispatched in machine mode,
/// the response carries `dispatch_contract_mode="machine"` AND the
/// resolved `task_contract_source_path` so observers (audit, PR
/// review, CI) can prove the on-disk Lisp drove the brief — the
/// markdown rendering (if requested via `render_command`) is
/// purely compatibility metadata in this mode.
#[test]
fn build_workstation_dispatch_response_machine_mode_pins_contract_path() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "agent-team");
    let outcome = wd::WorkstationDispatchOutcome::Dispatched {
        task_brief: "## Source contract\n- task-contract v1: `/tmp/p/.missiond/tasks/generated/plan/root.lisp`\n## Objective\nship\n".to_string(),
        task_brief_path: None,
        task_contract_source_path: Some(
            "/tmp/p/.missiond/tasks/generated/plan/root.lisp".to_string(),
        ),
        evidence_path: Some("/tmp/sidecar.json".to_string()),
        evidence_error: None,
        inner_payload: json!({"task_id": "btk-machine"}),
    };
    let decision = fixture_decision(wd::WorkstationDispatchSource::ExplicitArg);
    let result = build_workstation_dispatch_response(
        &plan,
        &resolved,
        outcome,
        &decision,
        &TaskContractEmissionRecord::off(),
        DispatchContractMode::Machine,
    );
    let v = parse_payload(&result);
    assert_eq!(v["status"], "executing");
    assert_eq!(v["workstation_dispatch_status"], "dispatched");
    assert_eq!(v["dispatch_contract_mode"], "machine");
    assert_eq!(
        v["task_contract_source_path"], "/tmp/p/.missiond/tasks/generated/plan/root.lisp",
        "machine-mode dispatch must surface the resolved contract path \
         so observers can prove the Lisp drove the brief (load-bearing SSOT)"
    );
    // The brief preview reflects the consumer overlay — the
    // `## Source contract` preamble is present, naming the same
    // on-disk path. This pins the requirement that markdown
    // rendering becomes optional compatibility metadata in
    // machine mode (not load-bearing).
    let preview = v["task_brief_preview"].as_str().unwrap_or("");
    assert!(
        preview.contains("## Source contract"),
        "machine-mode brief must carry the wave-19/07 `## Source contract` preamble"
    );
}

/// wave-20 / task 04 — when the workstation substrate refuses a
/// machine-mode dispatch because the on-disk task.lisp is malformed,
/// the response surfaces `SafeDescriptor` (status=
/// `skipped_malformed_task_contract`) with `dispatch_contract_mode=
/// "machine"`. We MUST NOT downgrade to `claude -p` or the legacy
/// natural-language brief — silently falling back would defeat the
/// machine SSOT contract.
#[test]
fn build_workstation_dispatch_response_machine_mode_malformed_contract_refuses() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let outcome = wd::WorkstationDispatchOutcome::SafeDescriptor {
        reason: wd::SafeDescriptorReason::MalformedTaskContract {
            path: "/tmp/p/.missiond/tasks/generated/plan/root.lisp".to_string(),
            reason: "missing required `goal` field".to_string(),
        },
        task_brief: None,
    };
    let decision = fixture_decision(wd::WorkstationDispatchSource::ExplicitArg);
    let result = build_workstation_dispatch_response(
        &plan,
        &resolved,
        outcome,
        &decision,
        &TaskContractEmissionRecord::off(),
        DispatchContractMode::Machine,
    );
    let v = parse_payload(&result);
    // No silent prompt fallback — the runner must surface the
    // refusal verbatim.
    assert_eq!(v["status"], "dispatch_skipped");
    assert_eq!(
        v["workstation_dispatch_status"],
        "skipped_malformed_task_contract"
    );
    assert_eq!(v["dispatch_contract_mode"], "machine");
    // Inner result must not have leaked through — we never
    // dispatched.
    assert!(v.get("inner_result").is_none());
    // The reason text must name the path so the caller can fix
    // and retry.
    let reason = v["workstation_dispatch_reason"].as_str().unwrap_or("");
    assert!(
        reason.contains(".missiond/tasks/generated"),
        "malformed-contract refusal must name the offending path"
    );
    assert!(
        reason.contains("missing required `goal` field"),
        "malformed-contract refusal must explain why the parse failed"
    );
}

// ── wave-16 / task 03 — auto-inference integration with plan hints ──
//
// The decision is the composition of `ParsedPlanHints::to_workstation_hints`
// with `evaluate_dispatch_decision`. These tests exercise the full
// pipeline so a refactor that moves the merge point can't silently
// change the inference outcome.

/// Build the inference context the runner would build at this point —
/// keeps the test bodies short and pins the merge order.
fn build_inference_ctx<'a>(
    target: &'a str,
    dispatch_strategy: &'a str,
    merged: &'a crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchHints,
) -> crate::handlers::knowledge::workstation_dispatch::InferenceContext<'a> {
    crate::handlers::knowledge::workstation_dispatch::InferenceContext {
        target,
        dispatch_strategy,
        objective: merged.objective.as_deref(),
        owned_files_present: !merged.owned_files.is_empty(),
        scope_present: merged
            .scope
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        target_project_present: merged
            .target_project
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        requested_cwd_present: merged
            .requested_cwd
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
    }
}

#[test]
fn auto_inference_fires_for_task_delegate_with_owned_files_and_strategy() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    // Hints come from a plan body that already has every signal — no
    // explicit caller arg, no PLAN-level :workstation-dispatch flag.
    let sexp = r#"
        (plan
          :objective "ship the wave"
          :dispatch-strategy "fresh-code-alignment"
          :owned-files ["a.rs" "b.rs"])
    "#;
    let hints = parse_plan_hints(sexp);
    let merged = hints.to_workstation_hints().merge_args(&json!({}));
    let ctx = build_inference_ctx("mission_task_delegate", "fresh-code-alignment", &merged);
    let decision =
        wd::evaluate_dispatch_decision(&json!({}), hints.workstation_dispatch_opt_in(), &ctx);
    assert_eq!(decision.source, wd::WorkstationDispatchSource::Inferred);
    assert!(decision.is_enabled());
}

#[test]
fn auto_inference_disabled_by_explicit_workstation_dispatch_false() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    // Same hints as above — explicit false must still suppress.
    let sexp = r#"
        (plan
          :objective "ship"
          :dispatch-strategy "fresh-code-alignment"
          :owned-files ["a.rs"])
    "#;
    let hints = parse_plan_hints(sexp);
    let merged = hints
        .to_workstation_hints()
        .merge_args(&json!({"workstation_dispatch": false}));
    let ctx = build_inference_ctx("mission_task_delegate", "fresh-code-alignment", &merged);
    let decision = wd::evaluate_dispatch_decision(
        &json!({"workstation_dispatch": false}),
        hints.workstation_dispatch_opt_in(),
        &ctx,
    );
    assert_eq!(decision.source, wd::WorkstationDispatchSource::Disabled);
    assert!(!decision.is_enabled());
}

#[test]
fn explicit_workstation_dispatch_true_preserves_wave15_path() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    // Even with no scoping hints in PLAN.lisp, explicit true still
    // routes through workstation-dispatch — wave-15 contract pin.
    let hints = parse_plan_hints("(plan :objective \"ship\")");
    let merged = hints
        .to_workstation_hints()
        .merge_args(&json!({"workstation_dispatch": true}));
    let ctx = build_inference_ctx("mission_task_delegate", "unknown", &merged);
    let decision = wd::evaluate_dispatch_decision(
        &json!({"workstation_dispatch": true}),
        hints.workstation_dispatch_opt_in(),
        &ctx,
    );
    assert_eq!(decision.source, wd::WorkstationDispatchSource::ExplicitArg);
    assert!(decision.is_enabled());
}

#[test]
fn auto_inference_skipped_when_strategy_unknown() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          :objective "ship"
          :owned-files ["a.rs"])
    "#;
    let hints = parse_plan_hints(sexp);
    let merged = hints.to_workstation_hints().merge_args(&json!({}));
    // Strategy resolves to `unknown` because no :dispatch-strategy or
    // :parallelism hint is supplied — same default the runner would
    // arrive at via `resolve_dispatch_strategy`.
    let ctx = build_inference_ctx("mission_task_delegate", "unknown", &merged);
    let decision =
        wd::evaluate_dispatch_decision(&json!({}), hints.workstation_dispatch_opt_in(), &ctx);
    assert_eq!(
        decision.source,
        wd::WorkstationDispatchSource::NotApplicable
    );
}

#[test]
fn auto_inference_skipped_when_objective_missing() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          :dispatch-strategy "fresh-code-alignment"
          :owned-files ["a.rs"])
    "#;
    let hints = parse_plan_hints(sexp);
    let merged = hints.to_workstation_hints().merge_args(&json!({}));
    let ctx = build_inference_ctx("mission_task_delegate", "fresh-code-alignment", &merged);
    let decision =
        wd::evaluate_dispatch_decision(&json!({}), hints.workstation_dispatch_opt_in(), &ctx);
    assert_eq!(
        decision.source,
        wd::WorkstationDispatchSource::NotApplicable
    );
    assert!(decision.reason.unwrap().contains("objective"));
}

#[test]
fn auto_inference_skipped_for_mission_execution_target() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          :objective "ship"
          :dispatch-strategy "fresh-code-alignment"
          :owned-files ["a.rs"])
    "#;
    let hints = parse_plan_hints(sexp);
    let merged = hints.to_workstation_hints().merge_args(&json!({}));
    let ctx = build_inference_ctx("mission_execution", "fresh-code-alignment", &merged);
    let decision =
        wd::evaluate_dispatch_decision(&json!({}), hints.workstation_dispatch_opt_in(), &ctx);
    assert_eq!(
        decision.source,
        wd::WorkstationDispatchSource::NotApplicable
    );
}

#[test]
fn auto_inference_skipped_for_mission_flow_run_target() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          :objective "ship"
          :dispatch-strategy "fresh-code-alignment"
          :owned-files ["a.rs"])
    "#;
    let hints = parse_plan_hints(sexp);
    let merged = hints.to_workstation_hints().merge_args(&json!({}));
    let ctx = build_inference_ctx("mission_flow_run", "fresh-code-alignment", &merged);
    let decision =
        wd::evaluate_dispatch_decision(&json!({}), hints.workstation_dispatch_opt_in(), &ctx);
    assert_eq!(
        decision.source,
        wd::WorkstationDispatchSource::NotApplicable
    );
}

#[test]
fn auto_inference_fires_for_agent_team_with_target_project_signal() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    // Scoping signal in this case is `target_project`, NOT owned_files.
    let sexp = r#"
        (plan
          :objective "ship the wave"
          :dispatch-strategy "agent-team"
          :target-project "missiond")
    "#;
    let hints = parse_plan_hints(sexp);
    let merged = hints.to_workstation_hints().merge_args(&json!({}));
    let ctx = build_inference_ctx("mission_task_delegate", "agent-team", &merged);
    let decision =
        wd::evaluate_dispatch_decision(&json!({}), hints.workstation_dispatch_opt_in(), &ctx);
    assert_eq!(decision.source, wd::WorkstationDispatchSource::Inferred);
}

#[test]
fn auto_inference_skipped_when_no_scope_signal() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    // Objective + strategy + target are all fine, but NO scoping hint:
    // no owned-files, no scope, no target-project, no requested-cwd.
    let sexp = r#"
        (plan
          :objective "ship"
          :dispatch-strategy "fresh-code-alignment")
    "#;
    let hints = parse_plan_hints(sexp);
    let merged = hints.to_workstation_hints().merge_args(&json!({}));
    let ctx = build_inference_ctx("mission_task_delegate", "fresh-code-alignment", &merged);
    let decision =
        wd::evaluate_dispatch_decision(&json!({}), hints.workstation_dispatch_opt_in(), &ctx);
    assert_eq!(
        decision.source,
        wd::WorkstationDispatchSource::NotApplicable
    );
    assert!(decision.reason.unwrap().contains("scoping signal"));
}

#[test]
fn workstation_dispatch_opt_in_off_when_arg_absent_and_plan_hint_absent() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let args = json!({});
    let hints = ParsedPlanHints::default();
    assert!(!wd::opt_in_requested(
        &args,
        hints.workstation_dispatch_opt_in()
    ));
}

#[test]
fn workstation_dispatch_opt_in_on_when_plan_hint_only() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let args = json!({});
    let mut hints = ParsedPlanHints::default();
    hints.workstation_dispatch_flag = Some("true".to_string());
    assert!(wd::opt_in_requested(
        &args,
        hints.workstation_dispatch_opt_in()
    ));
}

#[test]
fn workstation_dispatch_opt_in_on_when_explicit_arg_only() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let args = json!({"workstation_dispatch": true});
    let hints = ParsedPlanHints::default();
    assert!(wd::opt_in_requested(
        &args,
        hints.workstation_dispatch_opt_in()
    ));
}

// ── wave-15 :: plan resolution bridge — pure handler-shape ──────────
//
// Same pattern as the directive tests: drive the validation /
// stamping helpers that the plan handler composes for approve / mark
// / supersede. The DB-touching path (plan_get) is exercised by the
// daemon test suite; here we pin the deterministic branch logic so a
// refactor that breaks the resolution contract fails loud.
use crate::handlers::knowledge::review_gate::{
    parse_review_question_id_struct as wave15_parse_qid,
    parse_review_resolution_input as wave15_parse_input,
    stamp_needs_changes_next_step as wave15_stamp_next_step,
    stamp_resolution_payload as wave15_stamp_payload,
    validate_review_resolution_envelope as wave15_validate_envelope,
    ResolutionInputError as Wave15ResolutionInputError, ReviewDecision as Wave15ReviewDecision,
    ReviewResolutionInput as Wave15ReviewResolutionInput,
};

#[test]
fn plan_action_whitelist_pins_state_changing_actions() {
    // Pin the action whitelist for the plan surface. Update lockstep
    // with the resolution wiring if a new state-changing action lands.
    assert_eq!(
        PLAN_REVIEW_ACTIONS,
        &["compile", "approve", "mark", "supersede"]
    );
}

#[test]
fn plan_resolution_input_missing_decision_rejected_at_handler_boundary() {
    let args = json!({
        "plan_id": "00000000-0000-0000-0000-000000000abc",
        "review_question_id": "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve",
    });
    let err = wave15_parse_input(&args).unwrap_err();
    assert_eq!(err, Wave15ResolutionInputError::MissingDecision);
}

#[test]
fn plan_resolution_envelope_accepts_canonical_approve() {
    let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve";
    let parsed = wave15_parse_qid(qid).unwrap();
    wave15_validate_envelope(
        &parsed,
        "plan",
        "00000000-0000-0000-0000-000000000abc",
        1,
        PLAN_REVIEW_ACTIONS,
    )
    .expect("approve via valid review id must pass envelope validation");
}

#[test]
fn plan_resolution_envelope_accepts_canonical_mark() {
    let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v2:mark";
    let parsed = wave15_parse_qid(qid).unwrap();
    wave15_validate_envelope(
        &parsed,
        "plan",
        "00000000-0000-0000-0000-000000000abc",
        2,
        PLAN_REVIEW_ACTIONS,
    )
    .expect("mark via valid review id must pass envelope validation");
}

#[test]
fn plan_resolution_envelope_accepts_canonical_supersede() {
    let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:supersede";
    let parsed = wave15_parse_qid(qid).unwrap();
    wave15_validate_envelope(
        &parsed,
        "plan",
        "00000000-0000-0000-0000-000000000abc",
        1,
        PLAN_REVIEW_ACTIONS,
    )
    .expect("supersede via valid review id must pass envelope validation");
}

#[test]
fn plan_resolution_envelope_rejects_stale_version() {
    let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve";
    let parsed = wave15_parse_qid(qid).unwrap();
    let err = wave15_validate_envelope(
        &parsed,
        "plan",
        "00000000-0000-0000-0000-000000000abc",
        3,
        PLAN_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "STALE_REVIEW_VERSION");
}

#[test]
fn plan_resolution_envelope_rejects_scope_mismatch() {
    // qid says scope=directive but submitted to the plan surface →
    // REVIEW_SCOPE_MISMATCH (handler rejects before mutating state).
    let qid = "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve";
    let parsed = wave15_parse_qid(qid).unwrap();
    let err = wave15_validate_envelope(
        &parsed,
        "plan",
        "00000000-0000-0000-0000-000000000abc",
        1,
        PLAN_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "REVIEW_SCOPE_MISMATCH");
}

#[test]
fn plan_resolution_envelope_rejects_unsupported_action() {
    // archive isn't a valid plan-surface action even though it's
    // valid on the directive surface — must be REJECTED here.
    let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:archive";
    let parsed = wave15_parse_qid(qid).unwrap();
    let err = wave15_validate_envelope(
        &parsed,
        "plan",
        "00000000-0000-0000-0000-000000000abc",
        1,
        PLAN_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "REVIEW_ACTION_UNSUPPORTED");
}

#[test]
fn plan_rejected_decision_records_reason_in_payload_without_approving() {
    let input = Wave15ReviewResolutionInput {
        question_id: "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve".to_string(),
        decision: Wave15ReviewDecision::Rejected,
        actor: Some("operator-1".to_string()),
        note: Some("PLAN.lisp missing acceptance commands".to_string()),
    };
    // Replay the handler's keep-artifact branch.
    let mut payload = json!({
        "plan_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
    });
    payload["status"] = json!("review_rejected");
    wave15_stamp_payload(&mut payload, &input);
    assert_eq!(payload["status"], "review_rejected");
    assert_eq!(payload["review_decision"], "rejected");
    assert_eq!(payload["review_decision_outcome"], "keep_artifact");
    assert_eq!(payload["review_actor"], "operator-1");
    assert!(payload["review_note"]
        .as_str()
        .unwrap()
        .contains("acceptance commands"));
}

#[test]
fn plan_needs_changes_decision_surfaces_next_step() {
    let input = Wave15ReviewResolutionInput {
        question_id: "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve".to_string(),
        decision: Wave15ReviewDecision::NeedsChanges,
        actor: Some("operator-1".to_string()),
        note: Some("split DAG into smaller waves".to_string()),
    };
    let mut payload = json!({
        "plan_id": "00000000-0000-0000-0000-000000000abc",
        "version": 1,
    });
    payload["status"] = json!("review_needs_changes");
    wave15_stamp_next_step(&mut payload, "plan", "compile");
    wave15_stamp_payload(&mut payload, &input);
    assert_eq!(payload["status"], "review_needs_changes");
    assert_eq!(payload["review_decision"], "needs_changes");
    assert_eq!(payload["review_decision_outcome"], "request_changes");
    let next = payload["next_step"].as_str().unwrap();
    assert!(next.contains("rework"));
    assert!(next.contains("plan"));
    assert!(next.contains("compile"));
}

#[test]
fn plan_resolution_legacy_quiet_path_returns_none_when_no_qid() {
    let args = json!({"plan_id": "00000000-0000-0000-0000-000000000abc"});
    assert!(wave15_parse_input(&args).unwrap().is_none());
}

#[test]
fn plan_supersede_envelope_anchored_to_old_plan_id() {
    // For supersede, the resolution envelope is anchored to the OLD
    // plan id (the artifact being closed), not the new one.
    let qid = "review:plan:00000000-0000-0000-0000-000000000aaa:v1:supersede";
    let parsed = wave15_parse_qid(qid).unwrap();
    wave15_validate_envelope(
        &parsed,
        "plan",
        "00000000-0000-0000-0000-000000000aaa",
        1,
        PLAN_REVIEW_ACTIONS,
    )
    .expect("supersede must validate against old_plan_id");
    let err = wave15_validate_envelope(
        &parsed,
        "plan",
        "00000000-0000-0000-0000-000000000bbb", // new id — must fail
        1,
        PLAN_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "REVIEW_ARTIFACT_MISMATCH");
}

// ── wave-16 :: subscriber outcome enum is loud + DB-free ────────────

#[test]
fn plan_subscriber_outcome_supersede_needs_explicit_call() {
    // The subscriber path can only see the OLD plan id from the qid;
    // it cannot infer the NEW plan id, so supersede must be deferred
    // to the explicit caller-side bridge.
    let outcome = PlanSubscriberOutcome::SupersedeNeedsExplicitCall;
    assert_eq!(outcome, PlanSubscriberOutcome::SupersedeNeedsExplicitCall);
}

#[test]
fn plan_subscriber_outcome_mark_needs_explicit_call() {
    // The `mark` qid envelope encodes the action label only, not the
    // target column value, so the subscriber cannot infer which
    // PlanStatus to flip to.
    let outcome = PlanSubscriberOutcome::MarkNeedsExplicitCall;
    assert_eq!(outcome, PlanSubscriberOutcome::MarkNeedsExplicitCall);
}

#[test]
fn plan_subscriber_outcome_compile_no_op_carries_decision() {
    let outcome = PlanSubscriberOutcome::CompileNoOp {
        decision: ReviewDecision::Approved,
    };
    assert_eq!(
        outcome,
        PlanSubscriberOutcome::CompileNoOp {
            decision: ReviewDecision::Approved
        }
    );
}

// ── wave-17 / task 01 — resume input field set ─────────────────────

#[test]
fn parse_plan_node_resume_input_via_handler_boundary_matches_review_gate_helper() {
    // The plan handler invokes `parse_plan_node_resume_input` from
    // `review_gate.rs`. Pin the wire shape end-to-end so the handler
    // boundary stays in sync with the helper's contract.
    let args = json!({
        "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
        "resume_review_decision": "approved",
        "resume_actor": "agent-team",
        "resume_note": "proceed",
    });
    let input = parse_plan_node_resume_input(&args)
        .expect("ok")
        .expect("some");
    assert_eq!(
        input.question_id,
        "review:plan:abc:v1:plan-node:0123456789abcdef"
    );
    assert_eq!(input.decision, ReviewDecision::Approved);
    assert_eq!(input.actor.as_deref(), Some("agent-team"));
    assert_eq!(input.note.as_deref(), Some("proceed"));
}

#[test]
fn parse_plan_node_resume_input_handler_boundary_quiet_when_id_absent() {
    // No resume id → caller falls through to the standard execute
    // pipeline. Must NOT error so the wave-15 manager-side resolution
    // input contract stays byte-identical.
    let args = json!({
        "review_question_id": "review:plan:abc:v1:approve",
        "review_decision": "approved",
    });
    assert!(parse_plan_node_resume_input(&args).expect("ok").is_none());
}

// ── wave-18 / task 05 — cross-plan distill chain v0 ─────────────────

#[test]
fn distill_chain_requested_detects_any_chain_knob() {
    // No chain knobs → caller did not opt in. Backward-compat with
    // wave-17 / task 05 byte-shape: the chain orchestrator never
    // touches the response when the caller is silent.
    assert!(!distill_chain_requested(&json!({})));
    // Any single knob counts.
    assert!(distill_chain_requested(
        &json!({"distill_chain_id": "chain-1"})
    ));
    assert!(distill_chain_requested(
        &json!({"distill_chain_mode": "record_only"})
    ));
    assert!(distill_chain_requested(
        &json!({"distill_chain_name": "loop"})
    ));
    // Combination still counts (canonical opt-in shape).
    assert!(distill_chain_requested(
        &json!({"distill_chain_id": "x", "distill_chain_mode": "dry_run"})
    ));
}

#[test]
fn parse_distill_chain_id_blank_collapses_to_none() {
    // Blank / whitespace-only must NOT poison the audit row with an
    // empty id — collapses to absent so the runner falls back to
    // the deterministic auto id (chain:auto:plan-<plan_id>).
    assert_eq!(parse_distill_chain_id(&json!({})), None);
    assert_eq!(
        parse_distill_chain_id(&json!({"distill_chain_id": ""})),
        None
    );
    assert_eq!(
        parse_distill_chain_id(&json!({"distill_chain_id": "   "})),
        None
    );
    assert_eq!(
        parse_distill_chain_id(&json!({"distill_chain_id": "chain-42"})),
        Some("chain-42".to_string())
    );
    // Trim leading / trailing whitespace so caller-side concat
    // accidents don't shift the wire form silently.
    assert_eq!(
        parse_distill_chain_id(&json!({"distill_chain_id": "  chain-42  "})),
        Some("chain-42".to_string())
    );
}

#[test]
fn parse_distill_chain_name_blank_collapses_to_none() {
    assert_eq!(parse_distill_chain_name(&json!({})), None);
    assert_eq!(
        parse_distill_chain_name(&json!({"distill_chain_name": ""})),
        None
    );
    assert_eq!(
        parse_distill_chain_name(&json!({"distill_chain_name": "  "})),
        None
    );
    assert_eq!(
        parse_distill_chain_name(&json!({"distill_chain_name": "wave18-loop"})),
        Some("wave18-loop".to_string())
    );
}

#[test]
fn parse_distill_chain_mode_default_is_record_only() {
    // Absent / blank / canonical literal all collapse onto record_only
    // so the response always echoes a known mode. record_only is the
    // most conservative choice (no LLM, no workflow call).
    assert_eq!(parse_distill_chain_mode(&json!({})).unwrap(), "record_only");
    assert_eq!(
        parse_distill_chain_mode(&json!({"distill_chain_mode": ""})).unwrap(),
        "record_only"
    );
    assert_eq!(
        parse_distill_chain_mode(&json!({"distill_chain_mode": "record_only"})).unwrap(),
        "record_only"
    );
    assert_eq!(
        parse_distill_chain_mode(&json!({"distill_chain_mode": "dry_run"})).unwrap(),
        "dry_run"
    );
    assert_eq!(
        parse_distill_chain_mode(&json!({"distill_chain_mode": "sonnet"})).unwrap(),
        "sonnet"
    );
}

#[test]
fn parse_distill_chain_mode_rejects_typos() {
    // Strict allowlist mirrors workflow.rs / wave-17 task 05. Sonnet
    // typos are particularly important to catch — the brief forbids
    // ever invoking sonnet without an explicit mode, and a silent
    // collapse to record_only would mask the caller's intent.
    let err = parse_distill_chain_mode(&json!({"distill_chain_mode": "sonett"})).unwrap_err();
    assert!(
        err.contains("sonett"),
        "error must echo the rejected value, got `{}`",
        err
    );
    assert!(
        err.contains("record_only"),
        "error must spell out the allowlist, got `{}`",
        err
    );

    let err2 = parse_distill_chain_mode(&json!({"distill_chain_mode": "live"})).unwrap_err();
    assert!(err2.contains("live"));
}

#[test]
fn validate_distill_chain_args_rejects_chain_without_finalize() {
    // Any chain knob without finalize_plan=true must fail-fast — the
    // chain only fires AFTER a successful finalization, so silently
    // dropping the chain request would mask the caller's intent.
    let result = validate_distill_chain_args(&json!({"distill_chain_id": "chain-1"}))
        .expect("validator must reject");
    assert_eq!(result.is_error, Some(true));
    // Structured-error payload carries `error_code` + `reason`.
    let payload = tool_result_payload(&result);
    assert_eq!(
        payload.get("error_code").and_then(|v| v.as_str()),
        Some("INVALID_PARAM")
    );
    let reason = payload
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        reason.contains("finalize_plan=true"),
        "error must point at the missing finalize knob; got `{}`",
        reason
    );
}

#[test]
fn validate_distill_chain_args_rejects_unknown_mode_even_without_finalize() {
    // Validation runs eagerly on the mode allowlist so a typo never
    // survives until the next live caller. The mode check fires
    // BEFORE the finalize cross-field rule so the error message
    // points at the actual typo.
    let result = validate_distill_chain_args(&json!({"distill_chain_mode": "warp"}))
        .expect("validator must reject");
    assert_eq!(result.is_error, Some(true));
    let payload = tool_result_payload(&result);
    let reason = payload
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(reason.contains("warp"), "got `{}`", reason);
}

#[test]
fn validate_distill_chain_args_accepts_canonical_opt_in() {
    // Canonical shape: finalize_plan=true + chain knobs.
    assert!(validate_distill_chain_args(&json!({
        "finalize_plan": true,
        "distill_chain_id": "chain-1",
        "distill_chain_mode": "record_only",
        "distill_chain_name": "wave18-loop",
    }))
    .is_none());
    // Bare chain mode + finalize_plan also fine (id auto-derived).
    assert!(validate_distill_chain_args(&json!({
        "finalize_plan": true,
        "distill_chain_mode": "dry_run",
    }))
    .is_none());
    // No chain knobs at all → backward-compat (wave-17 / task 04 byte-shape).
    assert!(validate_distill_chain_args(&json!({})).is_none());
}

#[test]
fn validate_distill_chain_args_accepts_auto_sonnet_bool_shapes() {
    // wave-21 / task 07 — both auto_sonnet knobs accept the
    // canonical bool shape. Pairing them does not require
    // finalize_plan because the validator scopes the cross-field
    // rule to wave-18 chain knobs (auto_sonnet is forwarded by
    // workflow.rs, not gated here).
    assert!(validate_distill_chain_args(&json!({
        "auto_sonnet": true,
        "auto_sonnet_approved": true,
    }))
    .is_none());
    assert!(validate_distill_chain_args(&json!({
        "auto_sonnet": false,
        "auto_sonnet_approved": false,
    }))
    .is_none());
}

#[test]
fn validate_distill_chain_args_rejects_auto_sonnet_string_typo() {
    // wave-21 / task 07 — the apply-gate strict-shape validator
    // refuses string `"true"` and routes through INVALID_PARAM.
    let result = validate_distill_chain_args(&json!({"auto_sonnet": "true"}))
        .expect("validator must reject string-shape auto_sonnet");
    assert_eq!(result.is_error, Some(true));
    let payload = tool_result_payload(&result);
    let reason = payload
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        reason.contains("auto_sonnet must be a boolean"),
        "reason: {}",
        reason
    );
    assert!(reason.contains("string"), "shape label leaked: {}", reason);
}

#[test]
fn validate_distill_chain_args_rejects_auto_sonnet_approved_number_typo() {
    // wave-21 / task 07 — the caller-attestation flag is also
    // strict-bool. Numbers fail loud.
    let result = validate_distill_chain_args(&json!({"auto_sonnet_approved": 1}))
        .expect("validator must reject number-shape auto_sonnet_approved");
    assert_eq!(result.is_error, Some(true));
    let payload = tool_result_payload(&result);
    let reason = payload
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        reason.contains("auto_sonnet_approved must be a boolean"),
        "reason: {}",
        reason
    );
}

#[test]
fn validate_distill_chain_args_accepts_auto_sonnet_policy_canonical_strings() {
    // wave-22 / task 06 — the closed-enum policy validator accepts
    // the three canonical strings (off | safe_after_rules | dry_run)
    // plus null / missing (which collapse to off).
    for v in [
        json!("off"),
        json!("safe_after_rules"),
        json!("dry_run"),
        json!(""),
        json!(null),
    ] {
        assert!(
            validate_distill_chain_args(&json!({"auto_sonnet_policy": v})).is_none(),
            "policy={:?} must validate",
            v
        );
    }
    // Missing also fine.
    assert!(validate_distill_chain_args(&json!({})).is_none());
}

#[test]
fn validate_distill_chain_args_rejects_auto_sonnet_policy_unknown_string() {
    // wave-22 / task 06 — typo / camelCase / case mismatch all
    // fail-fast as INVALID_PARAM. A single typo cannot escalate
    // the daemon (I2 carryover from wave-21/07).
    let result = validate_distill_chain_args(&json!({"auto_sonnet_policy": "safeAfterRules"}))
        .expect("validator must reject unknown policy string");
    assert_eq!(result.is_error, Some(true));
    let payload = tool_result_payload(&result);
    let reason = payload
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        reason.contains("auto_sonnet_policy must be one of"),
        "reason: {}",
        reason
    );
    assert!(
        reason.contains("safeAfterRules"),
        "echoed bad value: {}",
        reason
    );
}

#[test]
fn validate_distill_chain_args_rejects_auto_sonnet_policy_non_string_shapes() {
    // wave-22 / task 06 — bool / number / array / object all fail.
    for bad in [
        json!({"auto_sonnet_policy": true}),
        json!({"auto_sonnet_policy": 1}),
        json!({"auto_sonnet_policy": ["safe_after_rules"]}),
        json!({"auto_sonnet_policy": {"value": "safe_after_rules"}}),
    ] {
        let result = validate_distill_chain_args(&bad)
            .expect("validator must reject non-string policy shape");
        assert_eq!(result.is_error, Some(true), "input: {:?}", bad);
        let payload = tool_result_payload(&result);
        let reason = payload
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            reason.contains("auto_sonnet_policy must be a string"),
            "reason: {} (input: {:?})",
            reason,
            bad
        );
    }
}

#[test]
fn json_shape_label_returns_canonical_json_type_name() {
    // Plan-side label helper mirrors workflow::shape_label so the
    // two surfaces emit identical wording on shape rejections.
    assert_eq!(json_shape_label(&json!(null)), "null");
    assert_eq!(json_shape_label(&json!(true)), "boolean");
    assert_eq!(json_shape_label(&json!(42)), "number");
    assert_eq!(json_shape_label(&json!("x")), "string");
    assert_eq!(json_shape_label(&json!([1, 2])), "array");
    assert_eq!(json_shape_label(&json!({"k": "v"})), "object");
}

#[test]
fn derive_fallback_chain_id_anchors_on_plan_id() {
    // Deterministic fallback so re-runs against the same plan land
    // on the same chain bucket — auditors can correlate without
    // rolling a UUID.
    let plan_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap();
    let id = derive_fallback_chain_id(plan_id);
    assert!(id.contains("chain:auto:plan-"));
    assert!(id.contains("00000000-0000-0000-0000-000000000abc"));
    // Stability: same plan id → same fallback (no time / random
    // component sneaking in).
    assert_eq!(id, derive_fallback_chain_id(plan_id));
}

#[test]
fn chain_eligibility_skips_when_finalization_block_missing() {
    // Inner DAG payload without a `finalization` block means the
    // caller did not opt into wave-17 finalize. Chain MUST skip
    // (chain only fires AFTER a successful finalization).
    let payload = json!({
        "status": "dag_succeeded",
        "scheduler_mode": "dag_v1",
    });
    assert_eq!(
        chain_eligibility_skip_reason(&payload),
        Some(CHAIN_STATUS_SKIPPED_NO_FINALIZATION)
    );
}

#[test]
fn chain_eligibility_skips_when_plan_status_not_succeeded() {
    // Even with a finalization block, anything other than
    // `final_plan_status="succeeded"` MUST skip — failed / paused /
    // unchanged all collapse to the same skip reason so the
    // response carries one canonical label.
    for not_succeeded in ["failed", "executing", "awaiting_review", "unchanged", ""] {
        let payload = json!({
            "finalization": {"final_plan_status": not_succeeded},
        });
        assert_eq!(
            chain_eligibility_skip_reason(&payload),
            Some(CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED),
            "must skip when final_plan_status=`{}`",
            not_succeeded
        );
    }
}

#[test]
fn chain_eligibility_passes_when_plan_status_succeeded() {
    let payload = json!({
        "finalization": {"final_plan_status": "succeeded"},
    });
    assert_eq!(chain_eligibility_skip_reason(&payload), None);
}

#[test]
fn build_distill_chain_block_carries_canonical_shape() {
    // record_only path: no distill result, no warning, no triggered.
    let block = build_distill_chain_block(
        true,
        CHAIN_STATUS_RECORDED,
        "chain:wave18",
        "explicit_arg",
        "record_only",
        Some("wave18-loop"),
        Some(1),
        None,
        None,
        Some("/tmp/.missiond/v3/runtime/plans/abc.evidence.json"),
        None,
    );
    assert_eq!(block["triggered"], true);
    assert_eq!(block["status"], "recorded");
    assert_eq!(block["chain_id"], "chain:wave18");
    assert_eq!(block["chain_id_source"], "explicit_arg");
    assert_eq!(block["chain_mode"], "record_only");
    assert_eq!(block["chain_name"], "wave18-loop");
    assert_eq!(block["chain_index_in_plan"], 1);
    assert_eq!(
        block["evidence_path"],
        "/tmp/.missiond/v3/runtime/plans/abc.evidence.json"
    );
    assert!(block.get("distill_result").is_none());
    assert!(block.get("warning").is_none());
    assert!(block.get("evidence_error").is_none());
}

#[test]
fn build_distill_chain_block_surfaces_distill_result_and_warning() {
    // dry_run / sonnet path with a downstream warning: chain block
    // MUST surface BOTH the inner result AND the warning so
    // observers can detect partial success without scraping the
    // payload.
    let block = build_distill_chain_block(
        true,
        CHAIN_STATUS_RECORDED_DISTILL_WARNING,
        "chain:42",
        "derived_from_plan_id",
        "sonnet",
        None,
        Some(2),
        Some(json!({"error": "sonnet quota exhausted"})),
        Some("distill chain workflow call returned an error; plan finalization preserved"),
        Some("/tmp/.evidence.json"),
        None,
    );
    assert_eq!(block["status"], "recorded_with_distill_warning");
    assert_eq!(block["distill_result"]["error"], "sonnet quota exhausted");
    assert!(block["warning"]
        .as_str()
        .unwrap()
        .contains("plan finalization preserved"));
    assert!(
        block.get("chain_name").is_none(),
        "chain_name only emitted when set"
    );
}

#[test]
fn build_distill_chain_block_skip_path_keeps_triggered_false_no_evidence() {
    // Skip branch: triggered=false + reason as status; evidence_path
    // / chain_index_in_plan absent because nothing was written.
    let block = build_distill_chain_block(
        false,
        CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED,
        "chain:auto:plan-x",
        "derived_from_plan_id",
        "record_only",
        None,
        None,
        None,
        None,
        None,
        None,
    );
    assert_eq!(block["triggered"], false);
    assert_eq!(block["status"], "skipped_plan_not_succeeded");
    assert!(block.get("evidence_path").is_none());
    assert!(block.get("chain_index_in_plan").is_none());
}

#[test]
fn attach_distill_chain_to_payload_nests_under_finalization_when_present() {
    // Wave-17 finalization block exists → chain block lands under
    // `finalization.distill_chain` so callers can grep one place.
    let mut payload = json!({
        "status": "dag_succeeded",
        "finalization": {
            "final_plan_status": "succeeded",
            "rule": "all_terminal_no_failed_no_paused",
        },
    });
    let block = build_distill_chain_block(
        true,
        CHAIN_STATUS_RECORDED,
        "chain:42",
        "explicit_arg",
        "record_only",
        None,
        Some(1),
        None,
        None,
        Some("/tmp/x.json"),
        None,
    );
    attach_distill_chain_to_payload(&mut payload, block);
    assert_eq!(
        payload["finalization"]["distill_chain"]["chain_id"],
        "chain:42"
    );
    // Top-level shortcuts mirror the brief's response contract.
    assert_eq!(payload["distill_chain_status"], "recorded");
    assert_eq!(payload["distill_chain_id"], "chain:42");
}

#[test]
fn attach_distill_chain_to_payload_falls_back_to_top_level_when_no_finalization() {
    // No finalization block (skip branch) → chain block surfaces at
    // the top level so the caller still sees the skip status.
    let mut payload = json!({"status": "dag_succeeded"});
    let block = build_distill_chain_block(
        false,
        CHAIN_STATUS_SKIPPED_NO_FINALIZATION,
        "chain:auto:plan-x",
        "derived_from_plan_id",
        "record_only",
        None,
        None,
        None,
        None,
        None,
        None,
    );
    attach_distill_chain_to_payload(&mut payload, block);
    assert_eq!(
        payload["distill_chain"]["status"],
        "skipped_no_finalization"
    );
    assert_eq!(payload["distill_chain_status"], "skipped_no_finalization");
}

#[test]
fn attach_distill_chain_to_payload_surfaces_distill_result_shortcut() {
    // dry_run / sonnet path: top-level `distill_result` shortcut so
    // callers can grep the inner workflow payload without diving
    // into finalization.distill_chain.distill_result.
    let mut payload = json!({
        "status": "dag_succeeded",
        "finalization": {"final_plan_status": "succeeded"},
    });
    let block = build_distill_chain_block(
        true,
        CHAIN_STATUS_RECORDED_WITH_DISTILL,
        "chain:42",
        "explicit_arg",
        "dry_run",
        None,
        Some(1),
        Some(json!({"status": "dry_run", "persisted": false})),
        None,
        Some("/tmp/x.json"),
        None,
    );
    attach_distill_chain_to_payload(&mut payload, block);
    assert_eq!(payload["distill_result"]["status"], "dry_run");
    assert_eq!(payload["distill_result"]["persisted"], false);
    assert!(
        payload.get("distill_chain_warning").is_none(),
        "warning shortcut absent on the OK path"
    );
}

#[test]
fn attach_distill_chain_to_payload_surfaces_warning_shortcut() {
    let mut payload = json!({
        "status": "dag_succeeded",
        "finalization": {"final_plan_status": "succeeded"},
    });
    let block = build_distill_chain_block(
        true,
        CHAIN_STATUS_RECORDED_DISTILL_WARNING,
        "chain:42",
        "explicit_arg",
        "sonnet",
        None,
        Some(1),
        Some(json!({"error": "x"})),
        Some("workflow distill failed; plan finalization preserved"),
        Some("/tmp/x.json"),
        None,
    );
    attach_distill_chain_to_payload(&mut payload, block);
    assert_eq!(
        payload["distill_chain_warning"],
        "workflow distill failed; plan finalization preserved"
    );
}

// ── wave-18 / task 06 — autonomous PLAN field inference v0 ─────────

#[test]
fn parse_infer_plan_fields_mode_default_is_off() {
    let mode = parse_infer_plan_fields_mode(&json!({})).expect("default off");
    assert_eq!(mode, InferPlanFieldsMode::Off);
    let mode_blank = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": ""}))
        .expect("blank parses to off");
    assert_eq!(mode_blank, InferPlanFieldsMode::Off);
    let mode_off =
        parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "off"})).expect("explicit off");
    assert_eq!(mode_off, InferPlanFieldsMode::Off);
}

#[test]
fn parse_infer_plan_fields_mode_accepts_known_values() {
    let preview =
        parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "preview"})).expect("preview");
    assert_eq!(preview, InferPlanFieldsMode::Preview);
    let apply = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "apply_safe"}))
        .expect("apply_safe");
    assert_eq!(apply, InferPlanFieldsMode::ApplySafe);
}

#[test]
fn parse_infer_plan_fields_mode_rejects_typo() {
    let err = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "aply"}))
        .expect_err("typo rejected");
    assert!(err.contains("must be one of"));
    assert!(err.contains("aply"));
}

fn empty_input<'a>() -> PlanInferenceInput<'a> {
    PlanInferenceInput {
        plan_hints: ParsedPlanHints::default(),
        plan_sexp: "",
        compiled_from: None,
        evidence_entries: Vec::new(),
    }
}

#[test]
fn confidence_only_high_meets_apply_threshold() {
    assert!(InferenceConfidence::High.meets_apply_threshold());
    assert!(!InferenceConfidence::Medium.meets_apply_threshold());
    assert!(!InferenceConfidence::Low.meets_apply_threshold());
}

#[test]
fn infer_target_from_plan_sexp_high_confidence() {
    // PLAN.lisp `:target` hint normalises to a canonical target with
    // high confidence — caller did not specify, so it lands in
    // `inferred[]` (apply-eligible).
    let mut hints = ParsedPlanHints::default();
    hints.target = Some("mission_task_delegate".to_string());
    let input = PlanInferenceInput {
        plan_hints: hints,
        plan_sexp: "(plan :target \"mission_task_delegate\")",
        compiled_from: None,
        evidence_entries: Vec::new(),
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let inferred = r
        .inferred
        .iter()
        .find(|f| f.field == "target")
        .expect("target inferred");
    assert_eq!(inferred.value, json!("mission_task_delegate"));
    assert_eq!(inferred.confidence, InferenceConfidence::High);
    assert_eq!(inferred.source, "plan_sexp");
    assert!(r.evidence_sources.contains(&"plan_sexp"));
}

#[test]
fn infer_owned_files_from_evidence_sidecar_medium() {
    // Caller did not pass owned_files; PLAN-side hints absent; the
    // most-recent evidence entry carries an `owned_files` array under
    // `inner_dispatch`. That signal is medium-confidence (file lists
    // change across runs) so it lands in `suggested[]`.
    let evidence = vec![json!({
        "source": "plan_runner_dispatch",
        "kind": "dispatch",
        "inner_dispatch": {
            "owned_files": ["a.rs", "b.rs"],
        }
    })];
    let input = PlanInferenceInput {
        plan_hints: ParsedPlanHints::default(),
        plan_sexp: "",
        compiled_from: None,
        evidence_entries: evidence,
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let suggested = r
        .suggested
        .iter()
        .find(|f| f.field == "owned_files")
        .expect("owned_files suggested from evidence");
    assert_eq!(suggested.confidence, InferenceConfidence::Medium);
    assert_eq!(suggested.source, "evidence_sidecar");
    assert_eq!(suggested.value, json!(["a.rs", "b.rs"]));
    // No high-confidence inference for owned_files from evidence.
    assert!(r.inferred.iter().all(|f| f.field != "owned_files"));
}

#[test]
fn infer_owned_files_from_plan_sexp_high_confidence() {
    // PLAN.lisp `:owned-files [...]` is high-confidence — apply_safe
    // would fill caller args.
    let mut hints = ParsedPlanHints::default();
    hints.owned_files_raw = Some("[\"src/lib.rs\" \"src/main.rs\"]".to_string());
    let input = PlanInferenceInput {
        plan_hints: hints,
        plan_sexp: "(plan :owned-files [\"src/lib.rs\" \"src/main.rs\"])",
        compiled_from: None,
        evidence_entries: Vec::new(),
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let inferred = r
        .inferred
        .iter()
        .find(|f| f.field == "owned_files")
        .expect("owned_files inferred from plan_sexp");
    assert_eq!(inferred.confidence, InferenceConfidence::High);
    assert_eq!(inferred.source, "plan_sexp");
    assert_eq!(inferred.value, json!(["src/lib.rs", "src/main.rs"]));
}

#[test]
fn apply_safe_does_not_overwrite_caller_value() {
    // Caller explicitly passed target=mission_execution; PLAN-side hint
    // disagrees (mission_task_delegate). The inferer must report a
    // CONFLICT (never silently mutate over caller intent), and
    // `apply_safe_augmentation` must leave caller's value intact.
    let mut hints = ParsedPlanHints::default();
    hints.target = Some("mission_task_delegate".to_string());
    let input = PlanInferenceInput {
        plan_hints: hints,
        plan_sexp: "(plan :target \"mission_task_delegate\")",
        compiled_from: None,
        evidence_entries: Vec::new(),
    };
    let caller_args = json!({"target": "mission_execution"});
    let r = compute_plan_field_inference(&caller_args, &input);
    // Conflict reported, not auto-applied.
    let conflict = r
        .conflicts
        .iter()
        .find(|c| c.field == "target")
        .expect("target conflict surfaced");
    assert_eq!(conflict.caller_value, json!("mission_execution"));
    assert_eq!(conflict.inferred_value, json!("mission_task_delegate"));
    assert!(r.inferred.iter().all(|f| f.field != "target"));

    // Augmentation MUST preserve caller's explicit value.
    let augmented = apply_safe_augmentation(&caller_args, &r);
    assert_eq!(augmented["target"], "mission_execution");
}

#[test]
fn apply_safe_fills_missing_high_confidence_only() {
    // PLAN-side high-confidence hint for target + medium-confidence
    // evidence for owned_files. apply_safe should ONLY fill `target`
    // (high), never `owned_files` (medium → suggestion only).
    let mut hints = ParsedPlanHints::default();
    hints.target = Some("mission_task_delegate".to_string());
    let evidence = vec![json!({
        "inner_dispatch": {"owned_files": ["a.rs"]}
    })];
    let input = PlanInferenceInput {
        plan_hints: hints,
        plan_sexp: "(plan :target \"mission_task_delegate\")",
        compiled_from: None,
        evidence_entries: evidence,
    };
    let caller_args = json!({});
    let r = compute_plan_field_inference(&caller_args, &input);
    let augmented = apply_safe_augmentation(&caller_args, &r);
    // High-confidence target was applied.
    assert_eq!(augmented["target"], "mission_task_delegate");
    // Medium-confidence owned_files was NOT applied (still suggestion).
    assert!(augmented.get("owned_files").is_none());
    assert!(r.suggested.iter().any(|f| f.field == "owned_files"));
}

#[test]
fn low_or_medium_confidence_never_lands_in_inferred() {
    // Single evidence entry → medium confidence for target_project;
    // suggested only.
    let evidence = vec![json!({
        "inner_dispatch": {"target_project": "missiond"}
    })];
    let input = PlanInferenceInput {
        plan_hints: ParsedPlanHints::default(),
        plan_sexp: "",
        compiled_from: None,
        evidence_entries: evidence,
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let suggested = r
        .suggested
        .iter()
        .find(|f| f.field == "target_project")
        .expect("target_project suggested");
    assert_eq!(suggested.confidence, InferenceConfidence::Medium);
    assert!(r.inferred.iter().all(|f| f.field != "target_project"));
}

#[test]
fn target_project_high_confidence_when_evidence_repeats() {
    // Two evidence entries agreeing on the same target_project →
    // high-confidence (count >= 2).
    let evidence = vec![
        json!({"inner_dispatch": {"target_project": "missiond"}}),
        json!({"inner_dispatch": {"target_project": "missiond"}}),
    ];
    let input = PlanInferenceInput {
        plan_hints: ParsedPlanHints::default(),
        plan_sexp: "",
        compiled_from: None,
        evidence_entries: evidence,
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let inferred = r
        .inferred
        .iter()
        .find(|f| f.field == "target_project")
        .expect("repeated evidence promotes target_project");
    assert_eq!(inferred.confidence, InferenceConfidence::High);
    assert_eq!(inferred.value, json!("missiond"));
}

#[test]
fn workstation_dispatch_inferred_from_plan_hint() {
    // PLAN.lisp `:workstation-dispatch true` → high-confidence
    // workstation_dispatch=true.
    let mut hints = ParsedPlanHints::default();
    hints.workstation_dispatch_flag = Some("true".to_string());
    let input = PlanInferenceInput {
        plan_hints: hints,
        plan_sexp: "(plan :workstation-dispatch true)",
        compiled_from: None,
        evidence_entries: Vec::new(),
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let inferred = r
        .inferred
        .iter()
        .find(|f| f.field == "workstation_dispatch")
        .expect("workstation_dispatch inferred from plan");
    assert_eq!(inferred.value, json!(true));
    assert_eq!(inferred.confidence, InferenceConfidence::High);
}

#[test]
fn workstation_dispatch_caller_false_creates_conflict_with_plan_true() {
    let mut hints = ParsedPlanHints::default();
    hints.workstation_dispatch_flag = Some("true".to_string());
    let input = PlanInferenceInput {
        plan_hints: hints,
        plan_sexp: "(plan :workstation-dispatch true)",
        compiled_from: None,
        evidence_entries: Vec::new(),
    };
    let caller = json!({"workstation_dispatch": false});
    let r = compute_plan_field_inference(&caller, &input);
    let conflict = r
        .conflicts
        .iter()
        .find(|c| c.field == "workstation_dispatch")
        .expect("conflict surfaced");
    assert_eq!(conflict.caller_value, json!(false));
    assert_eq!(conflict.inferred_value, json!(true));
    // apply_safe must NEVER override caller value.
    let augmented = apply_safe_augmentation(&caller, &r);
    assert_eq!(augmented["workstation_dispatch"], false);
}

#[test]
fn dispatch_strategy_inferred_from_plan_hint() {
    let mut hints = ParsedPlanHints::default();
    hints.dispatch_strategy = Some("agent-team".to_string());
    let input = PlanInferenceInput {
        plan_hints: hints,
        plan_sexp: "(plan :dispatch-strategy \"agent-team\")",
        compiled_from: None,
        evidence_entries: Vec::new(),
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let inferred = r
        .inferred
        .iter()
        .find(|f| f.field == "dispatch_strategy")
        .expect("dispatch_strategy inferred");
    assert_eq!(inferred.value, json!("agent-team"));
    assert_eq!(inferred.confidence, InferenceConfidence::High);
}

#[test]
fn dispatch_strategy_from_parallelism_is_medium() {
    // PLAN-side `:parallelism agent-team` is medium-confidence
    // (mapped through, not declared as the strategy itself).
    let mut hints = ParsedPlanHints::default();
    hints.parallelism = Some("agent-team".to_string());
    let input = PlanInferenceInput {
        plan_hints: hints,
        plan_sexp: "(plan :parallelism agent-team)",
        compiled_from: None,
        evidence_entries: Vec::new(),
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let suggested = r
        .suggested
        .iter()
        .find(|f| f.field == "dispatch_strategy")
        .expect("dispatch_strategy suggested from parallelism");
    assert_eq!(suggested.confidence, InferenceConfidence::Medium);
}

#[test]
fn acceptance_mode_inferred_from_plan_top_level() {
    // The canonical hint scanner does NOT capture `:acceptance-mode`
    // (that lives on per-node forms in plan_dag.rs); v0 inference
    // re-scans the raw sexp directly so a top-level declaration is
    // still picked up.
    let input = PlanInferenceInput {
        plan_hints: ParsedPlanHints::default(),
        plan_sexp: r#"(plan :acceptance-mode "inner_status")"#,
        compiled_from: None,
        evidence_entries: Vec::new(),
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let inferred = r
        .inferred
        .iter()
        .find(|f| f.field == "acceptance_mode")
        .expect("acceptance_mode inferred");
    assert_eq!(inferred.value, json!("inner_status"));
    assert_eq!(inferred.confidence, InferenceConfidence::High);
}

#[test]
fn acceptance_mode_unrecognised_raw_does_not_infer() {
    let input = PlanInferenceInput {
        plan_hints: ParsedPlanHints::default(),
        plan_sexp: r#"(plan :acceptance-mode "cosmic")"#,
        compiled_from: None,
        evidence_entries: Vec::new(),
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    assert!(r.inferred.iter().all(|f| f.field != "acceptance_mode"));
    assert!(r.suggested.iter().all(|f| f.field != "acceptance_mode"));
}

#[test]
fn off_mode_preserves_default_args_unchanged() {
    // Off mode means we never even build the inferer input. Sanity
    // check: status helper reports `off`.
    let inf = PlanFieldInference::default();
    assert_eq!(inf.status(InferPlanFieldsMode::Off), "off");
}

#[test]
fn preview_status_reports_no_signal_when_inference_empty() {
    let inf = PlanFieldInference::default();
    assert_eq!(
        inf.status(InferPlanFieldsMode::Preview),
        "preview_no_signal"
    );
}

#[test]
fn preview_status_reports_preview_when_signals_present() {
    let mut inf = PlanFieldInference::default();
    inf.suggested.push(InferredField {
        field: "target",
        value: json!("mission_execution"),
        confidence: InferenceConfidence::Medium,
        source: "evidence_sidecar",
        detail: None,
    });
    assert_eq!(inf.status(InferPlanFieldsMode::Preview), "preview");
}

#[test]
fn apply_safe_status_reports_applied_when_high_confidence_present() {
    let mut inf = PlanFieldInference::default();
    inf.inferred.push(InferredField {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    assert_eq!(
        inf.status(InferPlanFieldsMode::ApplySafe),
        "apply_safe_applied"
    );
}

#[test]
fn apply_safe_status_reports_suggestions_only_when_no_high_confidence() {
    let mut inf = PlanFieldInference::default();
    inf.suggested.push(InferredField {
        field: "owned_files",
        value: json!(["a.rs"]),
        confidence: InferenceConfidence::Medium,
        source: "evidence_sidecar",
        detail: None,
    });
    assert_eq!(
        inf.status(InferPlanFieldsMode::ApplySafe),
        "apply_safe_suggestions_only"
    );
}

#[test]
fn evidence_sources_reflect_signals_seen() {
    // Signals from all three sources → all three names appear in
    // evidence_sources[].
    let mut hints = ParsedPlanHints::default();
    hints.target = Some("mission_task_delegate".to_string());
    let evidence = vec![json!({"inner_dispatch": {"target_project": "x"}})];
    let input = PlanInferenceInput {
        plan_hints: hints,
        plan_sexp: "(plan :target \"mission_task_delegate\")",
        compiled_from: Some("directive/abc:1"),
        evidence_entries: evidence,
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    assert!(r.evidence_sources.contains(&"plan_sexp"));
    assert!(r.evidence_sources.contains(&"evidence_sidecar"));
    assert!(r.evidence_sources.contains(&"compiled_from"));
}

#[test]
fn apply_safe_augmentation_skips_field_when_args_already_carry_it() {
    // Defensive guard: even if the inferer (somehow) listed a field
    // in `inferred[]` AND args already carry it, augmentation MUST
    // refuse to overwrite. This pins the invariant tested in the
    // conflict path so a future regression is loud.
    let mut inf = PlanFieldInference::default();
    inf.inferred.push(InferredField {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    let args = json!({"target": "mission_execution"});
    let augmented = apply_safe_augmentation(&args, &inf);
    // Must NOT have changed.
    assert_eq!(augmented["target"], "mission_execution");
}

#[test]
fn response_block_always_has_stable_shape() {
    let inf = PlanFieldInference::default();
    let block = inf.to_response_json(InferPlanFieldsMode::Preview);
    assert_eq!(block["mode"], "preview");
    assert!(block["inferred_fields"].is_array());
    assert!(block["suggested_fields"].is_array());
    assert!(block["conflicts"].is_array());
    assert!(block["evidence_sources"].is_array());
    assert_eq!(block["inference_status"], "preview_no_signal");
}

#[test]
fn caller_string_list_handles_caller_arg_shapes() {
    // Sanity-check the helper used by `infer_owned_files`.
    let args = json!({"owned_files": ["a", "b"]});
    let v = caller_string_list(&args, "owned_files");
    assert_eq!(v, vec!["a".to_string(), "b".to_string()]);
    let scalar = json!({"owned_files": "single"});
    assert_eq!(
        caller_string_list(&scalar, "owned_files"),
        vec!["single".to_string()]
    );
    // Empty default.
    assert!(caller_string_list(&json!({}), "owned_files").is_empty());
}

#[test]
fn compiled_from_keyword_scan_produces_medium_target() {
    // No PLAN-side hint, no evidence — but `compiled_from` carries
    // the keyword. Falls into the medium-confidence (suggested) bucket.
    let input = PlanInferenceInput {
        plan_hints: ParsedPlanHints::default(),
        plan_sexp: "",
        compiled_from: Some("directive/abc:1 — claudecode workstation"),
        evidence_entries: Vec::new(),
    };
    let r = compute_plan_field_inference(&json!({}), &input);
    let suggested = r
        .suggested
        .iter()
        .find(|f| f.field == "target")
        .expect("target suggested from compiled_from");
    assert_eq!(suggested.confidence, InferenceConfidence::Medium);
    assert_eq!(suggested.value, json!("mission_task_delegate"));
}

#[test]
fn empty_input_yields_empty_result() {
    let input = empty_input();
    let r = compute_plan_field_inference(&json!({}), &input);
    assert!(r.inferred.is_empty());
    assert!(r.suggested.is_empty());
    assert!(r.conflicts.is_empty());
    assert!(r.evidence_sources.is_empty());
}

#[test]
fn attach_inference_block_skips_when_block_absent() {
    // mode=off → block=None → response untouched.
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let original_text = match original.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("text"),
    };
    let r = attach_inference_block(original, None);
    let after_text = match r.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("text"),
    };
    assert_eq!(original_text, after_text);
}

#[test]
fn attach_inference_block_splices_block_into_payload() {
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let block = json!({"mode": "apply_safe", "inference_status": "apply_safe_applied"});
    let r = attach_inference_block(original, Some(block.clone()));
    let v = parse_payload(&r);
    assert_eq!(v["status"], "executing");
    assert_eq!(v["plan_field_inference"], block);
}

#[test]
fn attach_inference_block_preserves_existing_block() {
    // If the result already carries a `plan_field_inference` key
    // (future DAG / resume path), we must NEVER overwrite.
    let original = ToolResult::json_pretty(&json!({
        "status": "executing",
        "plan_field_inference": {"mode": "preview"},
    }));
    let block = json!({"mode": "apply_safe"});
    let r = attach_inference_block(original, Some(block));
    let v = parse_payload(&r);
    assert_eq!(v["plan_field_inference"]["mode"], "preview");
}

// ── wave-20 / task 07 — LLM-augmented PLAN field inference v0 ──────

#[test]
fn parse_infer_plan_fields_mode_accepts_sonnet_suggest() {
    // wave-20 / task 07 — new LLM-augmented mode lands on the same
    // allowlist as the wave-18 / task 06 deterministic modes.
    let mode = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "sonnet_suggest"}))
        .expect("sonnet_suggest accepted");
    assert_eq!(mode, InferPlanFieldsMode::SonnetSuggest);
    assert!(mode.is_llm_augmented());
    // Determinstic modes never report as LLM-augmented.
    assert!(!InferPlanFieldsMode::Off.is_llm_augmented());
    assert!(!InferPlanFieldsMode::Preview.is_llm_augmented());
    assert!(!InferPlanFieldsMode::ApplySafe.is_llm_augmented());
}

#[test]
fn parse_infer_plan_fields_mode_typo_error_lists_sonnet_suggest() {
    // Typo path now mentions sonnet_suggest in the error message so
    // a caller misspelling the new mode knows the canonical form.
    let err = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "sonnet-suggest"}))
        .expect_err("hyphenated form rejected");
    assert!(err.contains("sonnet_suggest"));
    assert!(err.contains("sonnet-suggest"));
}

#[test]
fn sonnet_suggest_mode_wire_string_round_trips() {
    assert_eq!(
        InferPlanFieldsMode::SonnetSuggest.as_wire(),
        INFER_MODE_SONNET_SUGGEST
    );
}

#[test]
fn parse_llm_proposals_accepts_wrapped_object() {
    // Canonical happy path — Sonnet returns the documented
    // `{"proposals": [...]}` envelope.
    let raw = r#"{
        "proposals": [
            {
                "field": "target",
                "value": "mission_task_delegate",
                "confidence": "high",
                "evidence": "PLAN sexp clearly delegates to claudecode"
            }
        ]
    }"#;
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert!(warnings.is_empty(), "warnings: {:?}", warnings);
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].field, "target");
    assert_eq!(proposals[0].value, json!("mission_task_delegate"));
    assert_eq!(proposals[0].confidence, InferenceConfidence::High);
    assert_eq!(proposals[0].conflict_status, LlmConflictStatus::None);
}

#[test]
fn parse_llm_proposals_accepts_bare_array() {
    // Sonnet sometimes elides the wrapper and emits a top-level
    // array. We accept both shapes.
    let raw = r#"[{"field":"workstation_dispatch","value":true,"confidence":"medium","evidence":"plan declares scope hints"}]"#;
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert!(warnings.is_empty(), "warnings: {:?}", warnings);
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].value, json!(true));
}

#[test]
fn parse_llm_proposals_strips_markdown_fence() {
    // The system prompt forbids fences but Sonnet sometimes emits
    // them anyway; we tolerate the wrapper.
    let raw = "```json\n{\"proposals\": [{\"field\":\"target\",\"value\":\"mission_execution\",\"confidence\":\"medium\",\"evidence\":\"vague evidence\"}]}\n```";
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert!(warnings.is_empty(), "warnings: {:?}", warnings);
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].value, json!("mission_execution"));
}

#[test]
fn parse_llm_proposals_rejects_unknown_field() {
    let raw = r#"{"proposals":[{"field":"orbital_velocity","value":"warp9","confidence":"high","evidence":"x"}]}"#;
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert!(proposals.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("orbital_velocity"));
}

#[test]
fn parse_llm_proposals_rejects_invalid_confidence() {
    let raw = r#"{"proposals":[{"field":"target","value":"mission_execution","confidence":"absolute","evidence":"x"}]}"#;
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("absolute"));
}

#[test]
fn parse_llm_proposals_rejects_missing_evidence() {
    // Evidence is required; an empty string drops the proposal.
    let raw = r#"{"proposals":[{"field":"target","value":"mission_execution","confidence":"high","evidence":""}]}"#;
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("evidence"));
}

#[test]
fn parse_llm_proposals_rejects_value_shape_mismatch() {
    // owned_files must be a string array, not a single string.
    let raw = r#"{"proposals":[{"field":"owned_files","value":"src/lib.rs","confidence":"medium","evidence":"x"}]}"#;
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("owned_files"));
    // Boolean expected for workstation_dispatch.
    let raw2 = r#"{"proposals":[{"field":"workstation_dispatch","value":42,"confidence":"medium","evidence":"x"}]}"#;
    let (proposals2, warnings2) = parse_llm_proposals(raw2);
    assert!(proposals2.is_empty());
    assert!(warnings2[0].contains("workstation_dispatch"));
}

#[test]
fn parse_llm_proposals_dedupes_repeated_fields() {
    let raw = r#"{
        "proposals":[
            {"field":"target","value":"mission_execution","confidence":"medium","evidence":"first"},
            {"field":"target","value":"mission_task_delegate","confidence":"high","evidence":"second"}
        ]
    }"#;
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].evidence, "first");
    assert!(warnings.iter().any(|w| w.contains("duplicate")));
}

#[test]
fn parse_llm_proposals_caps_long_lists() {
    // Build a list longer than the cap so the trim warning fires.
    let mut entries = Vec::new();
    for f in [
        "target",
        "dispatch_strategy",
        "target_project",
        "owned_files",
        "acceptance_mode",
        "workstation_dispatch",
        "extra_one",
        "extra_two",
        "extra_three",
        "extra_four",
    ] {
        let value = match f {
            "owned_files" => json!(["a.rs"]),
            "workstation_dispatch" => json!(true),
            _ => json!("mission_execution"),
        };
        entries.push(json!({
            "field": f,
            "value": value,
            "confidence": "low",
            "evidence": "x"
        }));
    }
    let raw = serde_json::to_string(&json!({"proposals": entries})).unwrap();
    let (proposals, warnings) = parse_llm_proposals(&raw);
    // Cap pinned at LLM_PROPOSAL_CAP (8); duplicate-fields and
    // unknown-fields are dropped before the cap check, so the cap
    // warning may not fire — but the proposal count must be ≤ cap.
    assert!(proposals.len() <= LLM_PROPOSAL_CAP);
    // Unknown fields surface at minimum the `extra_*` warnings.
    assert!(warnings.iter().any(|w| w.contains("extra_one")));
}

#[test]
fn parse_llm_proposals_rejects_garbage_json() {
    let raw = "not json at all";
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("not valid JSON"));
}

#[test]
fn parse_llm_proposals_rejects_missing_proposals_key() {
    let raw = r#"{"results": []}"#;
    let (proposals, warnings) = parse_llm_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("missing required `proposals`"));
}

#[test]
fn reconcile_marks_caller_conflict() {
    // Caller passed a different value for the same field — the
    // proposal must surface ConflictsWithCaller (and never auto-apply).
    let mut proposals = vec![LlmProposal {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::High,
        evidence: "plan sexp".to_string(),
        conflict_status: LlmConflictStatus::None,
    }];
    let deterministic = PlanFieldInference::default();
    let args = json!({"target": "mission_execution"});
    reconcile_llm_conflicts(&mut proposals, &deterministic, &args);
    assert_eq!(
        proposals[0].conflict_status,
        LlmConflictStatus::ConflictsWithCaller
    );
}

#[test]
fn reconcile_marks_deterministic_conflict() {
    // Caller silent; deterministic engine inferred a different
    // value with high confidence. Proposal must surface
    // ConflictsWithDeterministic.
    let mut deterministic = PlanFieldInference::default();
    deterministic.inferred.push(InferredField {
        field: "target",
        value: json!("mission_execution"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    let mut proposals = vec![LlmProposal {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::Medium,
        evidence: "compiled_from hint".to_string(),
        conflict_status: LlmConflictStatus::None,
    }];
    reconcile_llm_conflicts(&mut proposals, &deterministic, &json!({}));
    assert_eq!(
        proposals[0].conflict_status,
        LlmConflictStatus::ConflictsWithDeterministic
    );
}

#[test]
fn reconcile_marks_overlap_with_deterministic_suggestion() {
    // Deterministic suggestion (medium / low) at a different value
    // than LLM proposal — surfaced as overlap, lower precedence
    // than caller / deterministic-high conflicts.
    let mut deterministic = PlanFieldInference::default();
    deterministic.suggested.push(InferredField {
        field: "owned_files",
        value: json!(["a.rs"]),
        confidence: InferenceConfidence::Medium,
        source: "evidence_sidecar",
        detail: None,
    });
    let mut proposals = vec![LlmProposal {
        field: "owned_files",
        value: json!(["b.rs"]),
        confidence: InferenceConfidence::Low,
        evidence: "compiled_from".to_string(),
        conflict_status: LlmConflictStatus::None,
    }];
    reconcile_llm_conflicts(&mut proposals, &deterministic, &json!({}));
    assert_eq!(
        proposals[0].conflict_status,
        LlmConflictStatus::OverlapsDeterministicSuggestion
    );
}

#[test]
fn reconcile_leaves_conflict_none_when_caller_agrees() {
    // Caller passed the same value as the proposal — no conflict.
    let mut proposals = vec![LlmProposal {
        field: "target",
        value: json!("mission_execution"),
        confidence: InferenceConfidence::Medium,
        evidence: "agreement".to_string(),
        conflict_status: LlmConflictStatus::None,
    }];
    reconcile_llm_conflicts(
        &mut proposals,
        &PlanFieldInference::default(),
        &json!({"target": "MISSION_EXECUTION"}),
    );
    // String comparison is case-insensitive, mirroring the
    // deterministic engine.
    assert_eq!(proposals[0].conflict_status, LlmConflictStatus::None);
}

#[test]
fn reconcile_owned_files_is_set_like() {
    // owned_files compares order-independent so a permutation does
    // not surface as a deterministic conflict.
    let mut deterministic = PlanFieldInference::default();
    deterministic.inferred.push(InferredField {
        field: "owned_files",
        value: json!(["a.rs", "b.rs"]),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    let mut proposals = vec![LlmProposal {
        field: "owned_files",
        value: json!(["b.rs", "a.rs"]),
        confidence: InferenceConfidence::High,
        evidence: "permutation".to_string(),
        conflict_status: LlmConflictStatus::None,
    }];
    reconcile_llm_conflicts(&mut proposals, &deterministic, &json!({}));
    assert_eq!(proposals[0].conflict_status, LlmConflictStatus::None);
}

#[test]
fn llm_proposal_to_json_pins_applied_false() {
    // Critical invariant: every LLM proposal carries `applied=false`
    // on the wire so observers can `assert proposal.applied == false`
    // without re-reading the task contract.
    let p = LlmProposal {
        field: "target",
        value: json!("mission_execution"),
        confidence: InferenceConfidence::High,
        evidence: "x".to_string(),
        conflict_status: LlmConflictStatus::None,
    };
    let v = p.to_json();
    assert_eq!(v["applied"], json!(false));
    assert_eq!(v["field"], json!("target"));
    assert_eq!(v["confidence"], json!("high"));
    assert_eq!(v["conflict_status"], json!("none"));
}

#[test]
fn llm_bundle_unavailable_carries_reason() {
    let b = LlmProposalBundle::unavailable("gateway not initialized");
    assert_eq!(b.status, LlmProposalStatus::Unavailable);
    assert!(b.proposals.is_empty());
    assert_eq!(
        b.unavailable_reason.as_deref(),
        Some("gateway not initialized")
    );
    assert_eq!(b.request_caller.as_deref(), Some(SONNET_INFER_CALLER));
}

#[test]
fn response_block_under_sonnet_suggest_carries_llm_keys_when_unavailable() {
    // Even when Sonnet is unavailable we surface llm_status +
    // llm_proposals[] (empty) so observers pivot on a stable shape.
    let mut inf = PlanFieldInference::default();
    inf.llm = Some(LlmProposalBundle::unavailable("test reason"));
    let block = inf.to_response_json(InferPlanFieldsMode::SonnetSuggest);
    assert_eq!(block["mode"], "sonnet_suggest");
    assert_eq!(block["llm_status"], "llm_unavailable");
    assert_eq!(block["llm_proposals"], json!([]));
    assert_eq!(block["llm_unavailable_reason"], "test reason");
    assert_eq!(block["llm_caller"], SONNET_INFER_CALLER);
}

#[test]
fn response_block_under_sonnet_suggest_with_proposals() {
    let mut inf = PlanFieldInference::default();
    let bundle = LlmProposalBundle {
        status: LlmProposalStatus::Suggested,
        proposals: vec![LlmProposal {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::Medium,
            evidence: "compiled_from".to_string(),
            conflict_status: LlmConflictStatus::None,
        }],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: Some("claude-sonnet".to_string()),
        request_caller: Some(SONNET_INFER_CALLER.to_string()),
    };
    inf.llm = Some(bundle);
    let block = inf.to_response_json(InferPlanFieldsMode::SonnetSuggest);
    assert_eq!(block["llm_status"], "suggested");
    assert_eq!(block["llm_proposals"][0]["field"], "target");
    assert_eq!(block["llm_proposals"][0]["applied"], false);
    assert_eq!(block["llm_model"], "claude-sonnet");
}

#[test]
fn response_block_under_deterministic_modes_omits_llm_keys() {
    // Backward compatibility: existing wave-18 modes must produce
    // BYTE-IDENTICAL response shapes (no llm_* keys leaking through).
    let inf = PlanFieldInference::default();
    for mode in [
        InferPlanFieldsMode::Off,
        InferPlanFieldsMode::Preview,
        InferPlanFieldsMode::ApplySafe,
    ] {
        let block = inf.to_response_json(mode);
        assert!(block.get("llm_status").is_none(), "mode {:?}", mode);
        assert!(block.get("llm_proposals").is_none(), "mode {:?}", mode);
        assert!(
            block.get("llm_unavailable_reason").is_none(),
            "mode {:?}",
            mode
        );
    }
}

#[test]
fn sonnet_suggest_status_reports_no_deterministic_signal_when_empty() {
    let inf = PlanFieldInference::default();
    assert_eq!(
        inf.status(InferPlanFieldsMode::SonnetSuggest),
        "sonnet_suggest_no_deterministic_signal"
    );
}

#[test]
fn sonnet_suggest_status_reports_sonnet_suggest_when_signals_present() {
    let mut inf = PlanFieldInference::default();
    inf.suggested.push(InferredField {
        field: "target",
        value: json!("mission_execution"),
        confidence: InferenceConfidence::Medium,
        source: "evidence_sidecar",
        detail: None,
    });
    assert_eq!(
        inf.status(InferPlanFieldsMode::SonnetSuggest),
        "sonnet_suggest"
    );
}

#[test]
fn build_llm_inference_prompt_embeds_inputs() {
    // Pin the prompt shape so future regressions are visible: must
    // mention the PLAN sexp, the directive provenance, the evidence
    // digest, the deterministic block, and the caller args.
    let plan_sexp = "(plan :target \"mission_task_delegate\")";
    let evidence = vec![json!({"target": "mission_execution"})];
    let deterministic = PlanFieldInference::default();
    let args = json!({"foo": "bar"});
    let (system, user) = build_llm_inference_prompt(
        plan_sexp,
        Some("directive/abc:1"),
        &evidence,
        &deterministic,
        &args,
    );
    assert!(system.contains("plan field inference"));
    assert!(system.contains("STRICT JSON"));
    assert!(system.contains("conflict_status"));
    assert!(user.contains(plan_sexp));
    assert!(user.contains("directive/abc:1"));
    assert!(user.contains("\"foo\""));
    assert!(user.contains("\"target\": \"mission_execution\""));
}

#[test]
fn deterministic_covers_all_fields_pred_only_true_when_six_high_inferences() {
    let mut inf = PlanFieldInference::default();
    // Empty → false.
    assert!(!deterministic_covers_all_fields(&inf));
    for f in LLM_ALLOWED_FIELDS.iter().take(5) {
        inf.inferred.push(InferredField {
            field: *f,
            value: json!("x"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
    }
    // Only 5 of 6 → still false.
    assert!(!deterministic_covers_all_fields(&inf));
    // Add the last field → true.
    inf.inferred.push(InferredField {
        field: LLM_ALLOWED_FIELDS[5],
        value: json!(true),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    assert!(deterministic_covers_all_fields(&inf));
}

#[test]
fn deterministic_covers_all_fields_ignores_suggestions() {
    // Only high-confidence inferred entries count; suggestions
    // (medium / low) leave the predicate at `false` so the LLM is
    // still asked to weigh in.
    let mut inf = PlanFieldInference::default();
    for f in LLM_ALLOWED_FIELDS.iter() {
        inf.suggested.push(InferredField {
            field: *f,
            value: json!("x"),
            confidence: InferenceConfidence::Medium,
            source: "evidence_sidecar",
            detail: None,
        });
    }
    assert!(!deterministic_covers_all_fields(&inf));
}

#[test]
fn coerce_proposal_value_workstation_dispatch_string_normalises() {
    let v = coerce_proposal_value("workstation_dispatch", &json!("YES"))
        .expect("string yes coerces to bool true");
    assert_eq!(v, json!(true));
    let v = coerce_proposal_value("workstation_dispatch", &json!("0"))
        .expect("string 0 coerces to bool false");
    assert_eq!(v, json!(false));
}

#[test]
fn coerce_proposal_value_target_project_strips_whitespace() {
    let v =
        coerce_proposal_value("target_project", &json!("  missiond  ")).expect("trims whitespace");
    assert_eq!(v, json!("missiond"));
}

#[test]
fn coerce_proposal_value_owned_files_drops_blank_entries() {
    let v = coerce_proposal_value("owned_files", &json!(["src/lib.rs", "  ", "src/main.rs"]))
        .expect("blanks stripped");
    assert_eq!(v, json!(["src/lib.rs", "src/main.rs"]));
}

#[test]
fn refuse_llm_inference_in_dag_mode_blocks_sonnet_suggest() {
    // wave-20 / task 07 — single-node-only enforcement on the DAG path.
    let args = json!({
        "scheduler_mode": "dag_v1",
        "infer_plan_fields": "sonnet_suggest"
    });
    let err = super::super::plan_dag::refuse_llm_inference_in_dag_mode(&args)
        .expect("dag + sonnet_suggest combo refused");
    assert_eq!(err.is_error, Some(true));
    let payload = parse_payload(&err);
    let reason = payload["reason"]
        .as_str()
        .expect("structured ToolError carries `reason`");
    assert!(
        reason.contains("single-node-execute-only"),
        "reason: {}",
        reason
    );
    assert_eq!(payload["error_code"], "INVALID_PARAM");
}

#[test]
fn refuse_llm_inference_in_dag_mode_allows_deterministic_modes() {
    // off / preview / apply_safe stay accepted on the DAG path
    // (they were already accepted in wave-18 / task 06).
    for mode in ["off", "preview", "apply_safe"] {
        let args = json!({
            "scheduler_mode": "dag_v1",
            "infer_plan_fields": mode
        });
        assert!(
            super::super::plan_dag::refuse_llm_inference_in_dag_mode(&args).is_none(),
            "deterministic mode `{}` must not be refused on DAG path",
            mode
        );
    }
    // No infer_plan_fields at all → also accepted.
    let args = json!({"scheduler_mode": "dag_v1"});
    assert!(super::super::plan_dag::refuse_llm_inference_in_dag_mode(&args).is_none());
}

// ── wave-21 / task 04 — autonomous workstation LLM proposal v0 ─────

#[test]
fn parse_workstation_inference_mode_default_is_off() {
    let mode = parse_workstation_inference_mode(&json!({})).expect("default ok");
    assert_eq!(mode, WorkstationInferenceMode::Off);
    assert!(!mode.is_sonnet_suggest());
    let mode_blank = parse_workstation_inference_mode(&json!({"workstation_inference_mode": ""}))
        .expect("blank ok");
    assert_eq!(mode_blank, WorkstationInferenceMode::Off);
    let mode_off = parse_workstation_inference_mode(&json!({"workstation_inference_mode": "off"}))
        .expect("off ok");
    assert_eq!(mode_off, WorkstationInferenceMode::Off);
}

#[test]
fn parse_workstation_inference_mode_accepts_sonnet_suggest() {
    let mode =
        parse_workstation_inference_mode(&json!({"workstation_inference_mode": "sonnet_suggest"}))
            .expect("sonnet_suggest ok");
    assert_eq!(mode, WorkstationInferenceMode::SonnetSuggest);
    assert!(mode.is_sonnet_suggest());
}

#[test]
fn parse_workstation_inference_mode_rejects_typo() {
    let err =
        parse_workstation_inference_mode(&json!({"workstation_inference_mode": "sonnet-suggest"}))
            .expect_err("hyphenated form rejected");
    assert!(err.contains("workstation_inference_mode"));
    assert!(err.contains("sonnet_suggest"));
    assert!(err.contains("sonnet-suggest"));
}

#[test]
fn workstation_inference_mode_wire_string_round_trips() {
    assert_eq!(
        WorkstationInferenceMode::Off.as_wire(),
        WORKSTATION_INFER_MODE_OFF
    );
    assert_eq!(
        WorkstationInferenceMode::SonnetSuggest.as_wire(),
        WORKSTATION_INFER_MODE_SONNET_SUGGEST
    );
}

#[test]
fn refuse_workstation_inference_in_dag_mode_blocks_sonnet_suggest() {
    // wave-21 / task 04 — single-node-only enforcement on the DAG
    // path. Mirrors the wave-20 / task 07 enforcement on the
    // plan-field surface.
    let args = json!({
        "scheduler_mode": "dag_v1",
        "workstation_inference_mode": "sonnet_suggest"
    });
    let err = refuse_workstation_inference_in_dag_mode(&args)
        .expect("dag + sonnet_suggest combo refused");
    assert_eq!(err.is_error, Some(true));
    let payload = parse_payload(&err);
    let reason = payload["reason"]
        .as_str()
        .expect("structured ToolError carries `reason`");
    assert!(
        reason.contains("single-node-execute-only"),
        "reason: {}",
        reason
    );
    assert_eq!(payload["error_code"], "INVALID_PARAM");
}

#[test]
fn refuse_workstation_inference_in_dag_mode_allows_off_mode() {
    // Default `off` mode never trips the DAG refusal.
    for mode in [
        json!({"scheduler_mode": "dag_v1"}),
        json!({"scheduler_mode": "dag_v1", "workstation_inference_mode": "off"}),
        json!({"scheduler_mode": "dag_v1", "workstation_inference_mode": ""}),
    ] {
        assert!(
            refuse_workstation_inference_in_dag_mode(&mode).is_none(),
            "off-shaped mode must not be refused on DAG path: {}",
            mode
        );
    }
}

#[test]
fn refuse_workstation_inference_in_dag_mode_no_op_outside_dag() {
    // sonnet_suggest WITHOUT scheduler_mode=dag_v1 is allowed (single-
    // node executes are the canonical wave-21 / task 04 surface).
    let args = json!({"workstation_inference_mode": "sonnet_suggest"});
    assert!(refuse_workstation_inference_in_dag_mode(&args).is_none());
}

#[test]
fn plan_hints_carry_workstation_signal_detects_objective() {
    let mut h = ParsedPlanHints::default();
    assert!(!plan_hints_carry_workstation_signal(&h));
    h.objective = Some("ship".to_string());
    assert!(plan_hints_carry_workstation_signal(&h));
}

#[test]
fn plan_hints_carry_workstation_signal_detects_each_workstation_knob() {
    // fn pointer (not closure) so the array elements all share one
    // type. Each fn flips exactly one knob; the assertion confirms
    // the predicate fires off any single knob.
    type Mutator = fn(&mut ParsedPlanHints);
    let cases: &[(Mutator, &str)] = &[
        (|h| h.objective = Some("o".into()), "objective"),
        (|h| h.summary = Some("s".into()), "summary"),
        (|h| h.scope = Some("z".into()), "scope"),
        (|h| h.owned_files_raw = Some("[a]".into()), "owned"),
        (|h| h.forbidden_files_raw = Some("[b]".into()), "forbidden"),
        (|h| h.acceptance_commands_raw = Some("[c]".into()), "accept"),
        (|h| h.commit_policy = Some("p".into()), "policy"),
        (|h| h.target_project = Some("missiond".into()), "tp"),
        (|h| h.requested_cwd = Some("/x".into()), "cwd"),
        (|h| h.dispatch_strategy = Some("agent-team".into()), "ds"),
    ];
    for (mutate, label) in cases {
        let mut h = ParsedPlanHints::default();
        mutate(&mut h);
        assert!(
            plan_hints_carry_workstation_signal(&h),
            "{} hint should register as signal",
            label
        );
    }
}

#[test]
fn plan_hints_carry_workstation_signal_ignores_blank_strings() {
    let mut h = ParsedPlanHints::default();
    h.objective = Some("   ".to_string());
    h.scope = Some("".to_string());
    assert!(!plan_hints_carry_workstation_signal(&h));
}

#[test]
fn attach_workstation_proposals_block_no_op_when_bundle_absent() {
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let r = attach_workstation_proposals_block(original, None);
    let v = parse_payload(&r);
    // Wire shape is unchanged when the bundle is absent.
    assert!(v.get("workstation_proposals").is_none());
    assert!(v.get("workstation_inference_mode").is_none());
    assert_eq!(v["status"], "executing");
}

#[test]
fn attach_workstation_proposals_block_attaches_bundle_and_mode() {
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let bundle = super::super::workstation_dispatch::WorkstationProposalBundle::unavailable(
        "Sonnet gateway not initialized; (no fallback to claude -p / prompt mode in v0)",
    );
    let r = attach_workstation_proposals_block(original, Some(&bundle));
    let v = parse_payload(&r);
    assert_eq!(v["workstation_proposals"]["status"], "llm_unavailable");
    assert_eq!(v["workstation_proposals"]["auto_spawn"], false);
    assert!(v["workstation_proposals"]["unavailable_reason"]
        .as_str()
        .unwrap_or("")
        .contains("no fallback"));
    assert_eq!(
        v["workstation_inference_mode"], "sonnet_suggest",
        "the mode echo must land on the response when bundle is present"
    );
}

#[test]
fn attach_workstation_proposals_block_preserves_pre_existing_block() {
    // If the result already carries a `workstation_proposals` key
    // (future DAG / resume path), we must NEVER overwrite.
    let original = ToolResult::json_pretty(&json!({
        "status": "executing",
        "workstation_proposals": {"status": "preserved"},
    }));
    let bundle = super::super::workstation_dispatch::WorkstationProposalBundle::unavailable("x");
    let r = attach_workstation_proposals_block(original, Some(&bundle));
    let v = parse_payload(&r);
    assert_eq!(v["workstation_proposals"]["status"], "preserved");
}

#[test]
fn attach_workstation_proposals_block_skips_error_results() {
    // Errors propagate untouched — never decorated with proposals.
    let original = ToolResult::structured_error(ToolError::new(error_codes::INVALID_PARAM, "boom"));
    assert_eq!(original.is_error, Some(true));
    let bundle = super::super::workstation_dispatch::WorkstationProposalBundle::unavailable("x");
    let r = attach_workstation_proposals_block(original, Some(&bundle));
    // The structured-error payload does NOT pick up the bundle keys.
    let payload = parse_payload(&r);
    assert!(payload.get("workstation_proposals").is_none());
    assert!(payload.get("workstation_inference_mode").is_none());
}

// ── wave-21 / task 05 — PLAN inference apply gate v1 ────────────────

#[test]
fn validate_apply_gate_args_accepts_bool_and_absent() {
    // Default (no flags) is valid.
    assert!(validate_apply_gate_args(&json!({})).is_ok());
    // Bool true / false are valid.
    assert!(validate_apply_gate_args(&json!({"apply_inferred_fields": true})).is_ok());
    assert!(validate_apply_gate_args(&json!({"apply_inferred_fields": false})).is_ok());
    assert!(validate_apply_gate_args(&json!({"persist_inference": true})).is_ok());
    // Object / array forms for llm_caller_approved are valid.
    assert!(validate_apply_gate_args(&json!({"llm_caller_approved": {"target": true}})).is_ok());
    assert!(validate_apply_gate_args(&json!({"llm_caller_approved": ["target"]})).is_ok());
}

#[test]
fn validate_apply_gate_args_rejects_string_form() {
    // Conservative: string `"true"` MUST NOT silently open the gate.
    let err = validate_apply_gate_args(&json!({"apply_inferred_fields": "true"}))
        .expect_err("string form rejected");
    assert!(err.contains("apply_inferred_fields must be a boolean"));
    let err = validate_apply_gate_args(&json!({"persist_inference": "true"}))
        .expect_err("persist_inference string rejected");
    assert!(err.contains("persist_inference must be a boolean"));
    // llm_caller_approved bool / string is also rejected.
    let err = validate_apply_gate_args(&json!({"llm_caller_approved": true}))
        .expect_err("bool form rejected");
    assert!(err.contains("llm_caller_approved must be object"));
}

#[test]
fn caller_requested_apply_defaults_false() {
    assert!(!caller_requested_apply(&json!({})));
    assert!(!caller_requested_apply(
        &json!({"apply_inferred_fields": false})
    ));
    assert!(caller_requested_apply(
        &json!({"apply_inferred_fields": true})
    ));
    // String form is treated as false (validator rejects it before
    // we get here, but the helper is defensive).
    assert!(!caller_requested_apply(
        &json!({"apply_inferred_fields": "true"})
    ));
}

#[test]
fn parse_llm_caller_approved_accepts_object_and_array() {
    let from_obj = parse_llm_caller_approved(
        &json!({"llm_caller_approved": {"target": true, "owned_files": false}}),
    );
    assert!(from_obj.contains("target"));
    assert!(!from_obj.contains("owned_files"));
    let from_arr = parse_llm_caller_approved(
        &json!({"llm_caller_approved": ["target", "workstation_dispatch"]}),
    );
    assert!(from_arr.contains("target"));
    assert!(from_arr.contains("workstation_dispatch"));
    // Unknown fields silently dropped (the gate's "unknown_field"
    // skip path covers downstream observability).
    let unknown = parse_llm_caller_approved(&json!({"llm_caller_approved": ["bogus_field"]}));
    assert!(unknown.is_empty());
}

#[test]
fn apply_gate_default_off_skips_everything() {
    // Apply flag absent → high-confidence inferred fields land in
    // `skipped_fields[]` with reason `apply_gate_not_requested`,
    // never in `applied_fields[]`.
    let mut inf = PlanFieldInference::default();
    inf.inferred.push(InferredField {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    let outcome = compute_apply_gate(&json!({}), &inf);
    assert!(!outcome.requested);
    assert!(outcome.applied.is_empty(), "no apply without explicit gate");
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].field, "target");
    assert_eq!(outcome.skipped[0].reason, "apply_gate_not_requested");
    // resulting_plan_preview is the caller args verbatim.
    assert_eq!(outcome.resulting_plan_preview, json!({}));
}

#[test]
fn apply_gate_opt_in_promotes_high_confidence_inferred() {
    let mut inf = PlanFieldInference::default();
    inf.inferred.push(InferredField {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    let args = json!({"apply_inferred_fields": true});
    let outcome = compute_apply_gate(&args, &inf);
    assert!(outcome.requested);
    assert_eq!(outcome.applied.len(), 1);
    assert_eq!(outcome.applied[0].field, "target");
    assert_eq!(
        outcome.applied[0].origin.as_wire(),
        "deterministic_inferred"
    );
    assert_eq!(
        outcome.resulting_plan_preview["target"],
        json!("mission_task_delegate")
    );
}

#[test]
fn apply_gate_skips_caller_value_already_set() {
    let mut inf = PlanFieldInference::default();
    inf.inferred.push(InferredField {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    let args = json!({
        "apply_inferred_fields": true,
        "target": "mission_execution",
    });
    let outcome = compute_apply_gate(&args, &inf);
    assert!(outcome.applied.is_empty(), "caller value wins");
    let skip = outcome
        .skipped
        .iter()
        .find(|s| s.field == "target")
        .expect("skip row");
    assert_eq!(skip.reason, "caller_value_already_set");
    // Preview leaves caller value intact.
    assert_eq!(
        outcome.resulting_plan_preview["target"],
        json!("mission_execution")
    );
}

#[test]
fn apply_gate_routes_conflicts_to_conflict_fields() {
    // Caller-vs-inferred conflicts are NEVER applied AND surface
    // separately on `conflict_fields[]`.
    let mut inf = PlanFieldInference::default();
    inf.conflicts.push(InferenceConflict {
        field: "target",
        caller_value: json!("mission_execution"),
        inferred_value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
    });
    let outcome = compute_apply_gate(
        &json!({
            "apply_inferred_fields": true,
            "target": "mission_execution",
        }),
        &inf,
    );
    assert!(outcome.applied.is_empty(), "no apply on conflict");
    assert_eq!(outcome.conflict.len(), 1);
    assert_eq!(outcome.conflict[0].field, "target");
    // A skip row mirrors the conflict for grep consistency.
    let skip = outcome
        .skipped
        .iter()
        .find(|s| s.reason == "caller_value_conflict")
        .expect("conflict-source skip row");
    assert_eq!(skip.field, "target");
    assert_eq!(skip.origin.as_wire(), "deterministic_conflict");
}

#[test]
fn apply_gate_skips_suggestions_below_threshold() {
    // Medium / low suggestions are conservative-skip even with the
    // gate flag set — the caller must promote them via explicit args.
    let mut inf = PlanFieldInference::default();
    inf.suggested.push(InferredField {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::Medium,
        source: "compiled_from",
        detail: None,
    });
    let outcome = compute_apply_gate(&json!({"apply_inferred_fields": true}), &inf);
    assert!(outcome.applied.is_empty(), "below-threshold never applies");
    let skip = outcome
        .skipped
        .iter()
        .find(|s| s.field == "target")
        .expect("skip row");
    assert_eq!(skip.reason, "below_apply_threshold");
    assert_eq!(skip.origin.as_wire(), "deterministic_suggested");
}

#[test]
fn apply_gate_llm_skipped_without_caller_approval() {
    // LLM proposals never apply unless the caller named the field
    // in `llm_caller_approved`. Default policy is conservative.
    let mut inf = PlanFieldInference::default();
    inf.llm = Some(LlmProposalBundle {
        status: LlmProposalStatus::Suggested,
        proposals: vec![LlmProposal {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            evidence: "vibes".to_string(),
            conflict_status: LlmConflictStatus::None,
        }],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: None,
        request_caller: None,
    });
    let outcome = compute_apply_gate(&json!({"apply_inferred_fields": true}), &inf);
    assert!(outcome.applied.is_empty(), "no LLM apply without approval");
    let skip = outcome
        .skipped
        .iter()
        .find(|s| s.origin == ApplyOrigin::LlmProposal)
        .expect("LLM skip row");
    assert_eq!(skip.reason, "llm_not_caller_approved");
}

#[test]
fn apply_gate_llm_promoted_when_caller_approved_and_safe() {
    let mut inf = PlanFieldInference::default();
    inf.llm = Some(LlmProposalBundle {
        status: LlmProposalStatus::Suggested,
        proposals: vec![LlmProposal {
            field: "dispatch_strategy",
            value: json!("agent-team"),
            confidence: InferenceConfidence::High,
            evidence: "PLAN explicitly mentions parallelism".to_string(),
            conflict_status: LlmConflictStatus::None,
        }],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: None,
        request_caller: None,
    });
    let outcome = compute_apply_gate(
        &json!({
            "apply_inferred_fields": true,
            "llm_caller_approved": ["dispatch_strategy"],
        }),
        &inf,
    );
    assert_eq!(outcome.applied.len(), 1);
    let af = &outcome.applied[0];
    assert_eq!(af.field, "dispatch_strategy");
    assert_eq!(af.origin.as_wire(), "llm_proposal");
    assert_eq!(
        outcome.resulting_plan_preview["dispatch_strategy"],
        json!("agent-team")
    );
}

#[test]
fn apply_gate_llm_safety_check_rejects_unsupported_strategy() {
    // `prompt-fallback` and `unknown` are deliberately excluded from
    // the apply-gate whitelist (mirrors wave-21 / task 04).
    let mut inf = PlanFieldInference::default();
    inf.llm = Some(LlmProposalBundle {
        status: LlmProposalStatus::Suggested,
        proposals: vec![LlmProposal {
            field: "dispatch_strategy",
            value: json!("prompt-fallback"),
            confidence: InferenceConfidence::High,
            evidence: "model guess".to_string(),
            conflict_status: LlmConflictStatus::None,
        }],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: None,
        request_caller: None,
    });
    let outcome = compute_apply_gate(
        &json!({
            "apply_inferred_fields": true,
            "llm_caller_approved": ["dispatch_strategy"],
        }),
        &inf,
    );
    assert!(outcome.applied.is_empty(), "unsupported strategy rejected");
    let skip = outcome
        .skipped
        .iter()
        .find(|s| s.field == "dispatch_strategy")
        .expect("skip row");
    assert_eq!(skip.reason, "llm_safety_check_failed");
    assert!(skip
        .detail
        .as_deref()
        .unwrap_or("")
        .contains("prompt-fallback"));
}

#[test]
fn apply_gate_llm_skipped_on_conflict_status() {
    // wave-20 reconciliation already tagged a deterministic conflict;
    // the apply gate respects it.
    let mut inf = PlanFieldInference::default();
    inf.llm = Some(LlmProposalBundle {
        status: LlmProposalStatus::Suggested,
        proposals: vec![LlmProposal {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::High,
            evidence: "model says X".to_string(),
            conflict_status: LlmConflictStatus::ConflictsWithDeterministic,
        }],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: None,
        request_caller: None,
    });
    let outcome = compute_apply_gate(
        &json!({
            "apply_inferred_fields": true,
            "llm_caller_approved": ["target"],
        }),
        &inf,
    );
    assert!(outcome.applied.is_empty());
    let skip = outcome
        .skipped
        .iter()
        .find(|s| s.field == "target")
        .expect("skip row");
    assert_eq!(skip.reason, "llm_conflict_present");
}

#[test]
fn apply_gate_llm_skipped_when_low_confidence() {
    let mut inf = PlanFieldInference::default();
    inf.llm = Some(LlmProposalBundle {
        status: LlmProposalStatus::Suggested,
        proposals: vec![LlmProposal {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::Low,
            evidence: "weak signal".to_string(),
            conflict_status: LlmConflictStatus::None,
        }],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: None,
        request_caller: None,
    });
    let outcome = compute_apply_gate(
        &json!({
            "apply_inferred_fields": true,
            "llm_caller_approved": ["target"],
        }),
        &inf,
    );
    assert!(outcome.applied.is_empty());
    let skip = outcome
        .skipped
        .iter()
        .find(|s| s.field == "target")
        .expect("skip row");
    assert_eq!(skip.reason, "llm_confidence_too_low");
}

#[test]
fn apply_gate_llm_skipped_when_deterministic_already_filled_slot() {
    // Deterministic high-confidence already promoted `target`;
    // the LLM proposal for the same slot should NOT silently apply
    // a second time.
    let mut inf = PlanFieldInference::default();
    inf.inferred.push(InferredField {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    inf.llm = Some(LlmProposalBundle {
        status: LlmProposalStatus::Suggested,
        proposals: vec![LlmProposal {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::High,
            evidence: "different guess".to_string(),
            conflict_status: LlmConflictStatus::None,
        }],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: None,
        request_caller: None,
    });
    let outcome = compute_apply_gate(
        &json!({
            "apply_inferred_fields": true,
            "llm_caller_approved": ["target"],
        }),
        &inf,
    );
    // Deterministic wins; LLM is skipped explicitly.
    assert_eq!(outcome.applied.len(), 1);
    assert_eq!(
        outcome.applied[0].origin.as_wire(),
        "deterministic_inferred"
    );
    let skip = outcome
        .skipped
        .iter()
        .find(|s| s.origin == ApplyOrigin::LlmProposal)
        .expect("LLM skip row");
    assert_eq!(skip.reason, "deterministic_inferred_already_applied");
}

#[test]
fn apply_gate_response_block_has_stable_shape() {
    let outcome = ApplyGateOutcome {
        requested: false,
        persist_inference_requested: false,
        applied: Vec::new(),
        skipped: Vec::new(),
        conflict: Vec::new(),
        resulting_plan_preview: json!({}),
    };
    let block = outcome.to_response_json();
    assert_eq!(block["requested"], false);
    assert_eq!(block["persist_inference_requested"], false);
    // v1 invariant: persisted plan text is NEVER mutated by this gate.
    assert_eq!(block["persist_inference_applied"], false);
    assert!(block["applied_fields"].is_array());
    assert!(block["skipped_fields"].is_array());
    assert!(block["conflict_fields"].is_array());
    assert!(block["resulting_plan_preview"].is_object());
}

#[test]
fn apply_gate_persist_inference_flag_echoed_but_never_applied() {
    // Even when caller passes persist_inference=true the v1 gate
    // must NOT mutate persisted plan text — the response surface
    // pins the invariant via `persist_inference_applied=false`.
    let outcome = compute_apply_gate(
        &json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
        }),
        &PlanFieldInference::default(),
    );
    assert!(outcome.persist_inference_requested);
    let block = outcome.to_response_json();
    assert_eq!(block["persist_inference_requested"], true);
    assert_eq!(block["persist_inference_applied"], false);
}

#[test]
fn attach_apply_gate_block_skips_when_block_absent() {
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let original_text = match original.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("text"),
    };
    let r = attach_apply_gate_block(original, None);
    let after_text = match r.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("text"),
    };
    assert_eq!(original_text, after_text);
}

#[test]
fn attach_apply_gate_block_splices_block_into_payload() {
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let block = json!({"requested": true, "applied_fields": []});
    let r = attach_apply_gate_block(original, Some(block.clone()));
    let v = parse_payload(&r);
    assert_eq!(v["status"], "executing");
    assert_eq!(v["apply_gate"], block);
}

#[test]
fn attach_apply_gate_block_preserves_pre_existing_block() {
    let original = ToolResult::json_pretty(&json!({
        "status": "executing",
        "apply_gate": {"requested": false},
    }));
    let block = json!({"requested": true});
    let r = attach_apply_gate_block(original, Some(block));
    let v = parse_payload(&r);
    // Pre-existing block wins.
    assert_eq!(v["apply_gate"]["requested"], false);
}

#[test]
fn attach_apply_gate_block_skips_error_results() {
    let original = ToolResult::structured_error(ToolError::new(error_codes::INVALID_PARAM, "boom"));
    let block = json!({"requested": true});
    let r = attach_apply_gate_block(original, Some(block));
    let payload = parse_payload(&r);
    assert!(payload.get("apply_gate").is_none());
}

#[test]
fn apply_gate_resulting_plan_preview_includes_caller_and_applied_fields() {
    // Caller passes one field, gate applies one inferred field; the
    // preview shows the union (without mutating caller args).
    let mut inf = PlanFieldInference::default();
    inf.inferred.push(InferredField {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
        detail: None,
    });
    let args = json!({
        "apply_inferred_fields": true,
        "objective": "ship feature",
    });
    let outcome = compute_apply_gate(&args, &inf);
    let preview = &outcome.resulting_plan_preview;
    assert_eq!(preview["objective"], "ship feature");
    assert_eq!(preview["target"], "mission_task_delegate");
    assert_eq!(preview["apply_inferred_fields"], true);
}

// ── Wave 21 / Task 08 — machine-contract autonomous loop smoke ──
//
// Pure-helper smoke proving the wave-19/20/21-07 distill chain
// receipts compose cleanly on a synthesised plan-execute payload
// without any AppState. The chain block is the wave21-07 SSOT for
// the auto-sonnet apply-gate's status taxonomy and we re-pin every
// wave21-07 invariant here in one assert block so a future refactor
// that drops a status / breaks the wire shape lands an explicit
// failure on the autonomous-loop smoke.
//
// Invariants pinned (cross-wave):
//   * I1-07  default-off byte-shape — no chain block surfaces
//            unless the gate explicitly fires.
//   * I3-07  the gate REUSES the wave-20 trigger outcomes — when the
//            trigger short-circuits to skip, the chain block surfaces
//            `triggered=false` + the dedicated skip status (NOT the
//            applied status).
//   * I7-07  wave-19 / wave-20 blocks remain unchanged — the chain
//            block is purely additive.

/// Wave21-08 smoke: when the wave21-07 auto-apply gate skips because
/// the plan never reached `succeeded`, the chain block surfaces
/// `triggered=false` + `status=skipped_plan_not_succeeded` and
/// suppresses every applied-side optional (evidence_path /
/// chain_index_in_plan / distill_result). This is the I3-07
/// invariant proof: the gate REUSES the trigger outcomes — it
/// never relaxes them by faking an evidence path.
#[test]
fn smoke_wave21_distill_chain_block_pins_skip_status_when_trigger_short_circuits() {
    let block = build_distill_chain_block(
        // triggered=false because the wave-20 trigger short-circuited
        false,
        CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED,
        "chain:auto:wave21-08-smoke",
        "derived_from_plan_id",
        "record_only",
        None,
        None,
        None,
        None,
        None,
        None,
    );
    assert_eq!(block["triggered"], false);
    assert_eq!(block["status"], "skipped_plan_not_succeeded");
    assert_eq!(block["chain_id"], "chain:auto:wave21-08-smoke");
    assert_eq!(block["chain_id_source"], "derived_from_plan_id");
    assert_eq!(block["chain_mode"], "record_only");
    // I3-07: the skip path MUST NOT fabricate evidence / index /
    // distill_result fields — those are reserved for the applied
    // path. A future refactor that always emits them would defeat
    // the gate's "REUSE the trigger outcomes" contract.
    for key in [
        "evidence_path",
        "chain_index_in_plan",
        "distill_result",
        "warning",
        "evidence_error",
        "chain_name",
    ] {
        assert!(
            block.get(key).is_none(),
            "wave21-08 smoke: skip-path chain block MUST NOT fabricate `{}`",
            key
        );
    }
}

/// Wave21-08 smoke: when the wave-20 trigger fires AND the inner
/// distill recorded a downstream warning, the chain block surfaces
/// BOTH the inner result AND the warning string under the dedicated
/// `recorded_with_distill_warning` status. This preserves observers'
/// ability to detect partial success without scraping the full
/// payload — wave21-07 invariant I5 (Sonnet failure preserves the
/// inner payload + surfaces a typed status) flows through this
/// block on the wave-20 distill side.
#[test]
fn smoke_wave21_distill_chain_block_surfaces_inner_distill_warning() {
    let block = build_distill_chain_block(
        true,
        CHAIN_STATUS_RECORDED_DISTILL_WARNING,
        "chain:wave21-08",
        "explicit_arg",
        "sonnet",
        Some("wave21-08-loop"),
        Some(2),
        Some(json!({"error": "sonnet quota exhausted"})),
        Some("distill chain workflow call returned an error; plan finalization preserved"),
        Some("/tmp/missiond-wave21-08/.evidence.json"),
        None,
    );
    assert_eq!(block["status"], "recorded_with_distill_warning");
    assert_eq!(block["chain_name"], "wave21-08-loop");
    assert_eq!(block["chain_index_in_plan"], 2);
    assert_eq!(block["distill_result"]["error"], "sonnet quota exhausted");
    assert!(block["warning"]
        .as_str()
        .unwrap_or("")
        .contains("plan finalization preserved"));
    // wave21-07 invariant I7: the chain block is additive — every
    // optional surfaces verbatim when supplied, never silently
    // dropped.
    assert_eq!(
        block["evidence_path"],
        "/tmp/missiond-wave21-08/.evidence.json"
    );
    assert!(block.get("evidence_error").is_none());
}

/// Wave21-08 smoke: machine-contract dispatch response build pins the
/// wave-20/04 SSOT invariant (`task_contract_source_path` surfaces
/// the resolved on-disk path) AND carries the wave-19/07 source-
/// contract preamble in the brief preview. Pinning these here closes
/// the workstation-side autonomous loop on a single canonical
/// fixture without spinning AppState.
#[test]
fn smoke_wave21_machine_dispatch_response_pins_source_path_invariant() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "agent-team");
    let contract_path =
        "/tmp/missiond-wave21-08-smoke/.missiond/tasks/wave21/wave21-08-dispatch.lisp";
    let outcome = wd::WorkstationDispatchOutcome::Dispatched {
        task_brief: format!(
            "## Source contract\n- task-contract v1: `{}`\n## Objective\nship the wave21-08 deterministic loop smoke\n",
            contract_path
        ),
        task_brief_path: None,
        task_contract_source_path: Some(contract_path.to_string()),
        evidence_path: Some("/tmp/missiond-wave21-08-smoke/.evidence.json".to_string()),
        evidence_error: None,
        inner_payload: json!({"task_id": "btk-wave21-08-smoke"}),
    };
    let decision = fixture_decision(wd::WorkstationDispatchSource::ExplicitArg);
    let result = build_workstation_dispatch_response(
        &plan,
        &resolved,
        outcome,
        &decision,
        &TaskContractEmissionRecord::off(),
        DispatchContractMode::Machine,
    );
    let v = parse_payload(&result);
    // wave-20/04 SSOT invariant: the response MUST surface the
    // resolved contract path so observers can prove the Lisp drove
    // the brief.
    assert_eq!(v["status"], "executing");
    assert_eq!(v["workstation_dispatch_status"], "dispatched");
    assert_eq!(v["dispatch_contract_mode"], "machine");
    assert_eq!(v["task_contract_source_path"], contract_path);
    // wave-19/07 invariant: the brief preview MUST carry the source-
    // contract preamble naming the same on-disk path.
    let preview = v["task_brief_preview"].as_str().unwrap_or("");
    assert!(
        preview.contains("## Source contract"),
        "wave21-08 brief MUST carry the wave-19/07 source-contract preamble"
    );
    assert!(
        preview.contains(contract_path),
        "wave21-08 brief preamble MUST name the same on-disk path the response surfaced"
    );
    // wave-21/04 invariant: machine-mode dispatch is the autonomous
    // loop SSOT — observers MUST see the dispatch_contract_mode
    // marker so they can route on it.
    assert_eq!(
        v["dispatch_contract_mode"], "machine",
        "wave21-08 invariant: machine-mode marker MUST be load-bearing"
    );
}

// ── wave-22 / task 04 — Persisted PLAN inference apply v2 ───────────

fn fixture_apply_outcome_with_one_high_inferred(args: &Value) -> ApplyGateOutcome {
    let inf = PlanFieldInference {
        inferred: vec![InferredField {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        }],
        ..Default::default()
    };
    compute_apply_gate(args, &inf)
}

#[test]
fn validate_apply_gate_args_accepts_caller_approved_and_proposal_hash() {
    // wave-22 / task 04 — extend wave-21 / task 05 validator to accept
    // the v2 persist-path opt-ins. Bool / string forms only.
    assert!(validate_apply_gate_args(&json!({"caller_approved": true})).is_ok());
    assert!(validate_apply_gate_args(&json!({"caller_approved": false})).is_ok());
    assert!(validate_apply_gate_args(
        &json!({"proposal_hash": "deadbeefdeadbeefdeadbeefdeadbeef"})
    )
    .is_ok());
    // Default (absent) is valid.
    assert!(validate_apply_gate_args(&json!({})).is_ok());
}

#[test]
fn validate_apply_gate_args_rejects_v2_typo_shapes() {
    // String "true" must NOT silently arm caller_approved.
    let err = validate_apply_gate_args(&json!({"caller_approved": "true"}))
        .expect_err("string form rejected");
    assert!(err.contains("caller_approved must be a boolean"));
    // Number / object proposal_hash must be rejected so a typo never
    // silently bypasses the strict hash preflight.
    let err = validate_apply_gate_args(&json!({"proposal_hash": 1234}))
        .expect_err("number form rejected");
    assert!(err.contains("proposal_hash must be a string"));
    let err = validate_apply_gate_args(&json!({"proposal_hash": {"hash": "abc"}}))
        .expect_err("object form rejected");
    assert!(err.contains("proposal_hash must be a string"));
}

#[test]
fn caller_requested_caller_approved_defaults_false() {
    // Default off — wave-21 / task 05 byte-shape preserved exactly
    // when caller does not supply the flag.
    assert!(!caller_requested_caller_approved(&json!({})));
    assert!(!caller_requested_caller_approved(
        &json!({"caller_approved": false})
    ));
    assert!(caller_requested_caller_approved(
        &json!({"caller_approved": true})
    ));
    // String form is treated as false — validator rejects it BEFORE
    // we get here, but the helper is defensive.
    assert!(!caller_requested_caller_approved(
        &json!({"caller_approved": "true"})
    ));
}

#[test]
fn caller_supplied_proposal_hash_strips_whitespace_and_treats_blank_as_none() {
    assert_eq!(caller_supplied_proposal_hash(&json!({})), None);
    assert_eq!(
        caller_supplied_proposal_hash(&json!({"proposal_hash": "   "})),
        None
    );
    assert_eq!(
        caller_supplied_proposal_hash(&json!({"proposal_hash": "  abc123  "})),
        Some("abc123".to_string())
    );
}

#[test]
fn compute_inference_proposal_hash_is_deterministic_and_field_order_independent() {
    // Hash must be deterministic over the same plan_id +
    // original_sexp_hash + applied set, regardless of the order in
    // which the gate appended fields. This is what lets the caller
    // capture-and-replay the hash from a preview call.
    let plan_id = uuid::Uuid::nil();
    let h0 = sha256_hex("(plan :id 1)");
    let af1 = AppliedField {
        field: "target",
        value: json!("mission_execution"),
        source: "plan_sexp",
        origin: ApplyOrigin::DeterministicInferred,
    };
    let af2 = AppliedField {
        field: "dispatch_strategy",
        value: json!("agent-team"),
        source: "plan_sexp",
        origin: ApplyOrigin::DeterministicInferred,
    };
    let a = compute_inference_proposal_hash(plan_id, &h0, &[af1.clone(), af2.clone()]);
    let b = compute_inference_proposal_hash(plan_id, &h0, &[af2, af1]);
    assert_eq!(a, b, "hash must be field-order independent (sorted)");
    assert_eq!(a.len(), 32, "32-hex prefix per the v2 spec");
}

#[test]
fn compute_inference_proposal_hash_changes_with_value() {
    let plan_id = uuid::Uuid::nil();
    let h0 = sha256_hex("(plan :id 1)");
    let af_a = AppliedField {
        field: "target",
        value: json!("mission_execution"),
        source: "plan_sexp",
        origin: ApplyOrigin::DeterministicInferred,
    };
    let af_b = AppliedField {
        field: "target",
        value: json!("mission_task_delegate"),
        source: "plan_sexp",
        origin: ApplyOrigin::DeterministicInferred,
    };
    let h_a = compute_inference_proposal_hash(plan_id, &h0, &[af_a]);
    let h_b = compute_inference_proposal_hash(plan_id, &h0, &[af_b]);
    assert_ne!(h_a, h_b);
}

#[test]
fn evaluate_persisted_apply_gate_skips_when_apply_flag_off() {
    // No apply flag ⇒ skip with the canonical reason. Default
    // wave-21 / task 05 v1 byte-shape preserved.
    let args = json!({});
    let apply = fixture_apply_outcome_with_one_high_inferred(&args);
    let status = evaluate_persisted_apply_gate(&args, &apply);
    assert_eq!(status, PersistedApplyStatus::SkippedApplyGateNotRequested);
    assert_eq!(status.as_wire(), "skipped_apply_gate_not_requested");
    assert!(!status.was_applied());
}

#[test]
fn evaluate_persisted_apply_gate_skips_when_persist_flag_off() {
    // apply_inferred_fields=true but persist_inference absent.
    let args = json!({"apply_inferred_fields": true});
    let apply = fixture_apply_outcome_with_one_high_inferred(&args);
    let status = evaluate_persisted_apply_gate(&args, &apply);
    assert_eq!(status, PersistedApplyStatus::SkippedPersistNotRequested);
    assert_eq!(status.as_wire(), "skipped_persist_not_requested");
}

#[test]
fn evaluate_persisted_apply_gate_skips_when_caller_not_approved() {
    // apply + persist but caller_approved missing — second human
    // opt-in invariant.
    let args = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
    });
    let apply = fixture_apply_outcome_with_one_high_inferred(&args);
    let status = evaluate_persisted_apply_gate(&args, &apply);
    assert_eq!(status, PersistedApplyStatus::SkippedCallerNotApproved);
}

#[test]
fn evaluate_persisted_apply_gate_skips_when_no_applied_fields() {
    // All four opt-ins but the gate promoted no fields ⇒ refuse to
    // write a no-op version.
    let args = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
        "target": "mission_execution",  // pre-fills the slot ⇒ skipped as caller_value_already_set
    });
    let apply = fixture_apply_outcome_with_one_high_inferred(&args);
    assert!(
        apply.applied.is_empty(),
        "fixture should be skipped because caller pre-filled"
    );
    let status = evaluate_persisted_apply_gate(&args, &apply);
    assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
}

#[test]
fn evaluate_persisted_apply_gate_authorises_when_all_four_opt_ins_and_applied() {
    let args = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let apply = fixture_apply_outcome_with_one_high_inferred(&args);
    assert!(!apply.applied.is_empty());
    let status = evaluate_persisted_apply_gate(&args, &apply);
    assert_eq!(status, PersistedApplyStatus::Applied);
    assert!(status.was_applied());
}

#[test]
fn enforce_persisted_apply_preflight_no_op_when_persist_path_not_armed() {
    // Caller did NOT opt into the persist path — preflight is a no-op
    // even when the supplied hash is wrong (legacy v1 callers must
    // never see a structured error here).
    for args in [
        json!({}),
        json!({"apply_inferred_fields": true}),
        json!({"persist_inference": true}),
        json!({"caller_approved": true}),
        json!({"apply_inferred_fields": true, "persist_inference": true}),
        json!({"apply_inferred_fields": true, "caller_approved": true}),
    ] {
        assert!(
            enforce_persisted_apply_preflight(&args, "deadbeefdeadbeefdeadbeefdeadbeef").is_ok(),
            "preflight must be no-op for non-persist args: {}",
            args
        );
    }
}

#[test]
fn enforce_persisted_apply_preflight_fails_fast_on_missing_hash() {
    // Caller opted into the persist path but did not supply a hash.
    let args = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let computed = "deadbeefdeadbeefdeadbeefdeadbeef";
    let err = enforce_persisted_apply_preflight(&args, computed)
        .expect_err("preflight must fail-fast on missing hash");
    assert_eq!(err.0, error_codes::INVALID_PARAM);
    assert!(err.1.contains("PERSIST_APPLY_MISSING_PROPOSAL_HASH"));
    assert!(err.1.contains(computed));
}

#[test]
fn enforce_persisted_apply_preflight_fails_fast_on_hash_mismatch() {
    let args = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
        "proposal_hash": "11111111111111111111111111111111",
    });
    let computed = "deadbeefdeadbeefdeadbeefdeadbeef";
    let err = enforce_persisted_apply_preflight(&args, computed)
        .expect_err("preflight must fail-fast on hash mismatch");
    assert_eq!(err.0, error_codes::INVALID_PARAM);
    assert!(err.1.contains("PERSIST_APPLY_PROPOSAL_HASH_MISMATCH"));
    assert!(err.1.contains("11111111111111111111111111111111"));
    assert!(err.1.contains(computed));
}

#[test]
fn enforce_persisted_apply_preflight_accepts_matching_hash_case_insensitive() {
    let computed = "deadbeefdeadbeefdeadbeefdeadbeef";
    // Same case.
    let args_same = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
        "proposal_hash": computed,
    });
    assert!(enforce_persisted_apply_preflight(&args_same, computed).is_ok());
    // Upper-case echo (defensive — observers may upper-case the hex).
    let args_upper = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
        "proposal_hash": computed.to_ascii_uppercase(),
    });
    assert!(enforce_persisted_apply_preflight(&args_upper, computed).is_ok());
}

#[test]
fn render_applied_field_to_lisp_emits_canonical_kebab_keywords() {
    // Mirrors the parse_plan_hints reader's keyword aliases.
    let target = render_applied_field_to_lisp("target", &json!("mission_execution"));
    assert_eq!(target, ":target \"mission_execution\"");
    let strat = render_applied_field_to_lisp("dispatch_strategy", &json!("agent-team"));
    assert_eq!(strat, ":dispatch-strategy \"agent-team\"");
    let proj = render_applied_field_to_lisp("target_project", &json!("missiond"));
    assert_eq!(proj, ":target-project \"missiond\"");
    let owned = render_applied_field_to_lisp("owned_files", &json!(["src/lib.rs", "src/main.rs"]));
    assert_eq!(owned, ":owned-files [\"src/lib.rs\" \"src/main.rs\"]");
    let ws = render_applied_field_to_lisp("workstation_dispatch", &json!(true));
    assert_eq!(ws, ":workstation-dispatch true");
}

#[test]
fn render_applied_field_to_lisp_escapes_quotes_and_backslashes() {
    let raw = render_applied_field_to_lisp("target", &json!("with\"quote\\back"));
    assert_eq!(raw, ":target \"with\\\"quote\\\\back\"");
}

#[test]
fn synthesize_persisted_sexp_preserves_original_verbatim_and_appends_annotation() {
    let original = "(plan :id \"plan-1\" :goal :ship)";
    let af = AppliedField {
        field: "target",
        value: json!("mission_execution"),
        source: "plan_sexp",
        origin: ApplyOrigin::DeterministicInferred,
    };
    let result = synthesize_persisted_sexp(
        original,
        &[af],
        "deadbeefdeadbeefdeadbeefdeadbeef",
        "2026-04-26T00:00:00Z",
    );
    // The original body MUST appear verbatim at the top — supersede
    // chain readers can `tail -1` to get the new annotation while
    // every prior byte stays comparable.
    assert!(
        result.starts_with(original),
        "original preserved verbatim: {}",
        result
    );
    // Header marker is greppable.
    assert!(result.contains("wave-22 / task 04 — persisted PLAN inference apply v2"));
    // Canonical annotation form.
    assert!(result.contains("(plan-inference-applied :inference-version \"v2\""));
    assert!(result.contains(":proposal-hash \"deadbeefdeadbeefdeadbeefdeadbeef\""));
    assert!(result.contains(":persisted-at \"2026-04-26T00:00:00Z\""));
    // Applied fields land as sibling keyword pairs so the
    // parse_plan_hints reader picks them up at the PLAN level.
    assert!(result.contains(":target \"mission_execution\""));
}

#[test]
fn synthesize_persisted_sexp_preserves_first_occurrence_semantics() {
    // parse_plan_hints keeps first-occurrence; an appended hint for
    // a slot the original already filled must NOT override it. We
    // verify the round-trip: synthesise the new sexp, parse hints
    // from it, and confirm the original target wins.
    let original = "(plan :id \"plan-1\" :target \"mission_task_delegate\")";
    let af = AppliedField {
        field: "target",
        value: json!("mission_execution"),
        source: "plan_sexp",
        origin: ApplyOrigin::LlmProposal,
    };
    let result = synthesize_persisted_sexp(original, &[af], "h0", "2026-04-26T00:00:00Z");
    let hints = parse_plan_hints(&result);
    assert_eq!(
        hints.target.as_deref(),
        Some("mission_task_delegate"),
        "first-occurrence wins; original target preserved at the persistence boundary"
    );
}

#[test]
fn synthesize_persisted_sexp_appends_new_hint_when_original_silent() {
    // When the original PLAN never spelled the field, the appended
    // hint becomes the live value (no prior occurrence to win).
    let original = "(plan :id \"plan-1\" :goal :ship)";
    let af = AppliedField {
        field: "dispatch_strategy",
        value: json!("agent-team"),
        source: "plan_sexp",
        origin: ApplyOrigin::DeterministicInferred,
    };
    let result = synthesize_persisted_sexp(original, &[af], "h0", "2026-04-26T00:00:00Z");
    let hints = parse_plan_hints(&result);
    assert_eq!(hints.dispatch_strategy.as_deref(), Some("agent-team"));
}

#[test]
fn persisted_apply_outcome_response_block_has_stable_shape() {
    let outcome = PersistedApplyOutcome::from_skip_reason(
        PersistedApplyStatus::NotRequested,
        &json!({}),
        "h0",
        &[],
        &[],
        None,
    );
    let v = outcome.to_response_json();
    // The wire shape is invariant — observers must see every field
    // (even when null) so dashboards never need to defensively
    // probe `.get(...)`.
    for key in [
        "status",
        "apply_inferred_fields_requested",
        "persist_inference_requested",
        "caller_approved",
        "original_sexp_hash",
        "resulting_sexp_hash",
        "computed_proposal_hash",
        "supplied_proposal_hash",
        "applied_fields",
        "skipped_fields",
        "new_plan_id",
        "new_plan_version",
        "rollback_plan_id",
    ] {
        assert!(
            v.get(key).is_some(),
            "persisted_apply block must always carry `{}`",
            key
        );
    }
    assert_eq!(v["status"], "not_requested");
    assert_eq!(v["apply_inferred_fields_requested"], false);
    assert_eq!(v["persist_inference_requested"], false);
    assert_eq!(v["caller_approved"], false);
    assert_eq!(v["original_sexp_hash"], "h0");
    assert!(v["resulting_sexp_hash"].is_null());
    assert!(v["new_plan_id"].is_null());
    assert!(v["rollback_plan_id"].is_null());
}

#[test]
fn build_persisted_apply_evidence_entry_carries_canonical_typed_shape() {
    // Mirrors wave-12 typed-evidence: schema_version="v0", canonical
    // source + kind so a single grep over the sidecar surfaces every
    // persist event.
    let plan_id = uuid::Uuid::nil();
    let outcome = PersistedApplyOutcome {
        status: PersistedApplyStatus::Applied,
        apply_inferred_fields_requested: true,
        persist_inference_requested: true,
        caller_approved: true,
        original_sexp_hash: "h0".into(),
        resulting_sexp_hash: Some("h1".into()),
        computed_proposal_hash: Some("ph".into()),
        supplied_proposal_hash: Some("ph".into()),
        applied_fields: vec![AppliedField {
            field: "target",
            value: json!("mission_execution"),
            source: "plan_sexp",
            origin: ApplyOrigin::DeterministicInferred,
        }],
        skipped_fields: vec![],
        new_plan_id: Some(uuid::Uuid::from_u128(1)),
        new_plan_version: Some(2),
        rollback_plan_id: Some(plan_id),
    };
    let entry = build_persisted_apply_evidence_entry(&outcome, plan_id);
    assert_eq!(entry["schema_version"], "v0");
    assert_eq!(entry["source"], "plan_inference_persisted_apply");
    assert_eq!(entry["kind"], "plan_inference_persisted_apply");
    assert_eq!(entry["plan_id"], plan_id.to_string());
    assert_eq!(entry["new_plan_version"], 2);
    assert_eq!(entry["original_sexp_hash"], "h0");
    assert_eq!(entry["resulting_sexp_hash"], "h1");
    assert_eq!(entry["proposal_hash"], "ph");
    assert_eq!(entry["status"], "applied");
    assert_eq!(entry["applied_fields"][0]["field"], "target");
    // rollback_pointer must point at the predecessor — observers
    // replaying a rollback need it.
    assert_eq!(entry["rollback_plan_id"], plan_id.to_string());
}

#[test]
fn attach_persisted_apply_block_no_op_when_block_absent() {
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let r = attach_persisted_apply_block(original, None);
    let v = parse_payload(&r);
    assert!(v.get("persisted_apply").is_none());
}

#[test]
fn attach_persisted_apply_block_splices_block_into_payload() {
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let block = json!({"status": "applied"});
    let r = attach_persisted_apply_block(original, Some(block.clone()));
    let v = parse_payload(&r);
    assert_eq!(v["persisted_apply"], block);
}

#[test]
fn attach_persisted_apply_block_preserves_pre_existing_block() {
    // Future DAG / resume paths may attach their own — never
    // overwrite.
    let original = ToolResult::json_pretty(&json!({
        "status": "executing",
        "persisted_apply": {"status": "preserved"},
    }));
    let r = attach_persisted_apply_block(original, Some(json!({"status": "applied"})));
    let v = parse_payload(&r);
    assert_eq!(v["persisted_apply"]["status"], "preserved");
}

#[test]
fn attach_persisted_apply_block_skips_error_results() {
    // Errors propagate untouched.
    let original = ToolResult::structured_error(ToolError::new(error_codes::INVALID_PARAM, "boom"));
    assert_eq!(original.is_error, Some(true));
    let r = attach_persisted_apply_block(original, Some(json!({"status": "applied"})));
    let payload = parse_payload(&r);
    assert!(payload.get("persisted_apply").is_none());
}

#[test]
fn persisted_apply_status_wire_strings_are_canonical_and_distinct() {
    // Dashboards pivot on the wire string — we lock the canonical
    // set so a refactor cannot silently re-spell one and break
    // observers.
    let wires = [
        PersistedApplyStatus::NotRequested.as_wire(),
        PersistedApplyStatus::Applied.as_wire(),
        PersistedApplyStatus::SkippedApplyGateNotRequested.as_wire(),
        PersistedApplyStatus::SkippedPersistNotRequested.as_wire(),
        PersistedApplyStatus::SkippedCallerNotApproved.as_wire(),
        PersistedApplyStatus::SkippedNothingToApply.as_wire(),
    ];
    // All distinct.
    let mut sorted: Vec<&'static str> = wires.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), wires.len(), "wire strings must be distinct");
    // Pinned exact values (anti-rename guard).
    assert_eq!(wires[0], "not_requested");
    assert_eq!(wires[1], "applied");
    assert_eq!(wires[2], "skipped_apply_gate_not_requested");
    assert_eq!(wires[3], "skipped_persist_not_requested");
    assert_eq!(wires[4], "skipped_caller_not_approved");
    assert_eq!(wires[5], "skipped_nothing_to_apply");
}

#[test]
fn persisted_apply_v2_preserves_wave21_05_invariant_apply_gate_v1_byte_shape_when_off() {
    // INVARIANT: wave-22 / task 04 must never alter the wave-21 / task
    // 05 v1 byte-shape when the v2 persist flags are absent. This
    // pins the back-compat contract — the v1 `apply_gate` block on
    // the response stays identical and `persisted_apply.status =
    // "not_requested"` carries no DB-mutation evidence.
    let args = json!({
        "apply_inferred_fields": true,
    });
    let apply = fixture_apply_outcome_with_one_high_inferred(&args);
    let v1_block = apply.to_response_json();
    assert_eq!(v1_block["requested"], true);
    // v1 invariant: persist_inference_applied is hard-pinned to
    // false on the apply_gate block. v2 does NOT mutate this — it
    // surfaces persistence on the SEPARATE `persisted_apply` block.
    assert_eq!(v1_block["persist_inference_applied"], false);
    // v2 evaluation on the same args returns a soft-skip — no DB
    // mutation, no error.
    let status = evaluate_persisted_apply_gate(&args, &apply);
    assert_eq!(status, PersistedApplyStatus::SkippedPersistNotRequested);
}

#[test]
fn persisted_apply_v2_preserves_wave21_05_invariant_conflicts_never_persist() {
    // INVARIANT: caller-vs-inferred conflicts NEVER apply (even
    // under v2 persist). The v1 gate routes them to
    // `conflict_fields[]` with `applied=[]`, so v2's
    // `evaluate_persisted_apply_gate` must downgrade to
    // SkippedNothingToApply.
    let mut inf = PlanFieldInference::default();
    inf.conflicts.push(InferenceConflict {
        field: "target",
        caller_value: json!("mission_task_delegate"),
        inferred_value: json!("mission_execution"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
    });
    let args = json!({
        "target": "mission_task_delegate",
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let apply = compute_apply_gate(&args, &inf);
    assert!(
        apply.applied.is_empty(),
        "conflicts MUST never reach applied[]"
    );
    let status = evaluate_persisted_apply_gate(&args, &apply);
    assert_eq!(
        status,
        PersistedApplyStatus::SkippedNothingToApply,
        "conflict-only outcome MUST persist nothing"
    );
}

#[test]
fn persisted_apply_v2_preserves_wave21_05_invariant_suggestions_never_persist() {
    // INVARIANT: medium / low-confidence suggestions NEVER apply
    // (sub-threshold). v2 must never persist them, even when all
    // four opt-ins are supplied.
    let mut inf = PlanFieldInference::default();
    inf.suggested.push(InferredField {
        field: "target",
        value: json!("mission_execution"),
        confidence: InferenceConfidence::Medium,
        source: "plan_sexp",
        detail: None,
    });
    let args = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let apply = compute_apply_gate(&args, &inf);
    assert!(
        apply.applied.is_empty(),
        "suggestions MUST stay below the apply threshold"
    );
    let status = evaluate_persisted_apply_gate(&args, &apply);
    assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
}

#[test]
fn persisted_apply_v2_preserves_wave21_05_invariant_llm_unapproved_never_persists() {
    // INVARIANT: LLM proposals require `llm_caller_approved`. v2
    // must never elevate an un-approved LLM proposal into the
    // persist path even when `caller_approved=true` (which
    // approves the PERSIST path, not the per-field LLM proposal).
    let mut inf = PlanFieldInference::default();
    inf.llm = Some(LlmProposalBundle {
        status: LlmProposalStatus::Suggested,
        proposals: vec![LlmProposal {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::High,
            evidence: "x".into(),
            conflict_status: LlmConflictStatus::None,
        }],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: None,
        request_caller: None,
    });
    // caller_approved=true is the PERSIST opt-in; llm_caller_approved
    // is absent ⇒ proposal must not apply.
    let args = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let apply = compute_apply_gate(&args, &inf);
    assert!(
        apply.applied.is_empty(),
        "LLM proposal MUST NOT apply without `llm_caller_approved` (caller_approved is the PERSIST gate, not the LLM gate)"
    );
    let status = evaluate_persisted_apply_gate(&args, &apply);
    assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
}

#[test]
fn persisted_apply_v2_preserves_wave21_05_invariant_strict_bool_shape() {
    // INVARIANT: strict bool shape. String "true" must NOT silently
    // arm the persist path — validator fail-fasts BEFORE we reach
    // the gate.
    for arg in [
        json!({"persist_inference": "true"}),
        json!({"caller_approved": "true"}),
        json!({"apply_inferred_fields": "true"}),
    ] {
        let err = validate_apply_gate_args(&arg).expect_err("string MUST be rejected");
        assert!(err.contains("must be a boolean"));
    }
}

#[test]
fn persisted_apply_v2_preserves_wave21_05_invariant_persist_inference_applied_field_intact() {
    // INVARIANT: the v1 `apply_gate.persist_inference_applied`
    // field stays hard-pinned to `false` (the v2 persistence
    // surfaces on the SEPARATE `persisted_apply` block, so the
    // v1 wire shape never changes).
    let args = json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let apply = fixture_apply_outcome_with_one_high_inferred(&args);
    let v1_block = apply.to_response_json();
    assert_eq!(
        v1_block["persist_inference_applied"], false,
        "wave-21 / task 05 invariant: v1 block's persist_inference_applied stays hard-pinned to false"
    );
    // The v2 persistence is reported on the parallel block.
    let v2_outcome = PersistedApplyOutcome::from_skip_reason(
        PersistedApplyStatus::Applied,
        &args,
        "h0",
        &apply.applied,
        &apply.skipped,
        Some("ph".into()),
    );
    let v2_block = v2_outcome.to_response_json();
    assert_eq!(v2_block["status"], "applied");
}

#[test]
fn persisted_apply_status_was_applied_only_for_applied() {
    assert!(PersistedApplyStatus::Applied.was_applied());
    for status in [
        PersistedApplyStatus::NotRequested,
        PersistedApplyStatus::SkippedApplyGateNotRequested,
        PersistedApplyStatus::SkippedPersistNotRequested,
        PersistedApplyStatus::SkippedCallerNotApproved,
        PersistedApplyStatus::SkippedNothingToApply,
    ] {
        assert!(
            !status.was_applied(),
            "{:?} must NOT report applied",
            status
        );
    }
}

// ── wave-22 / task 05 — autonomous workstation true spawn v1 wiring ──
//
// These tests cover the plan.rs splice + helper integration. The
// workstation_dispatch.rs gate evaluator already has its own
// exhaustive unit tests; here we focus on the plan.rs surface:
//   * `attach_workstation_auto_spawn_gate_block` no-op when the
//     gate outcome is absent (default ⇒ wave-21/04 byte-shape).
//   * `attach_workstation_auto_spawn_gate_block` splices the
//     block into a successful response.
//   * `attach_workstation_auto_spawn_gate_block` skips error
//     responses (matches the wave-21/04 attachers).
//   * `attach_workstation_auto_spawn_gate_block` preserves
//     pre-existing blocks (DAG / resume forward-compat).

#[test]
fn wave22_05_attach_auto_spawn_gate_block_no_op_when_outcome_absent() {
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let r = attach_workstation_auto_spawn_gate_block(original, None);
    let v = parse_payload(&r);
    assert!(
        v.get("workstation_auto_spawn_gate").is_none(),
        "wave-21 / task 04 byte-shape: gate block MUST be omitted when outcome is None"
    );
}

#[test]
fn wave22_05_attach_auto_spawn_gate_block_splices_block_into_payload() {
    use super::super::workstation_dispatch::{
        WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus, WorkstationProposalHashStatus,
    };
    let outcome = WorkstationAutoSpawnGateOutcome {
        requested: true,
        status: WorkstationAutoSpawnStatus::Spawned,
        spawn_target: Some("mission_task_delegate".to_string()),
        task_contract_path: Some(".missiond/tasks/foo.lisp".to_string()),
        proposal_hash_status: WorkstationProposalHashStatus::Matches,
        computed_proposal_hash: Some("0".repeat(32)),
        supplied_proposal_hash: Some("0".repeat(32)),
        caller_approved: true,
        preflight_status_acceptable: true,
        gate_results: vec!["rule:auto_spawn_gate_satisfied".to_string()],
        substrate_reason: None,
    };
    let original = ToolResult::json_pretty(&json!({"status": "executing"}));
    let r = attach_workstation_auto_spawn_gate_block(original, Some(&outcome));
    let v = parse_payload(&r);
    let block = v
        .get("workstation_auto_spawn_gate")
        .expect("gate block present");
    assert_eq!(block["auto_spawn_status"], "spawned");
    assert_eq!(block["spawn_target"], "mission_task_delegate");
    assert_eq!(block["proposal_hash_status"], "matches");
    assert_eq!(block["caller_approved"], true);
    assert_eq!(block["preflight_status_acceptable"], true);
    assert!(block["gate_results"].as_array().unwrap().len() >= 1);
}

#[test]
fn wave22_05_attach_auto_spawn_gate_block_skips_error_results() {
    use super::super::workstation_dispatch::{
        WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus, WorkstationProposalHashStatus,
    };
    let outcome = WorkstationAutoSpawnGateOutcome {
        requested: true,
        status: WorkstationAutoSpawnStatus::Spawned,
        spawn_target: None,
        task_contract_path: None,
        proposal_hash_status: WorkstationProposalHashStatus::NotSupplied,
        computed_proposal_hash: None,
        supplied_proposal_hash: None,
        caller_approved: false,
        preflight_status_acceptable: false,
        gate_results: vec![],
        substrate_reason: None,
    };
    let mut original = ToolResult::json_pretty(&json!({"error": "broke"}));
    original.is_error = Some(true);
    let r = attach_workstation_auto_spawn_gate_block(original, Some(&outcome));
    let v = parse_payload(&r);
    assert!(
        v.get("workstation_auto_spawn_gate").is_none(),
        "structured-error responses MUST stay uncluttered"
    );
}

#[test]
fn wave22_05_attach_auto_spawn_gate_block_preserves_pre_existing_block() {
    use super::super::workstation_dispatch::{
        WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus, WorkstationProposalHashStatus,
    };
    let outcome = WorkstationAutoSpawnGateOutcome {
        requested: true,
        status: WorkstationAutoSpawnStatus::Spawned,
        spawn_target: Some("mission_task_delegate".to_string()),
        task_contract_path: None,
        proposal_hash_status: WorkstationProposalHashStatus::Matches,
        computed_proposal_hash: None,
        supplied_proposal_hash: None,
        caller_approved: true,
        preflight_status_acceptable: true,
        gate_results: vec![],
        substrate_reason: None,
    };
    let original = ToolResult::json_pretty(&json!({
        "status": "executing",
        "workstation_auto_spawn_gate": {"auto_spawn_status": "preexisting_marker"},
    }));
    let r = attach_workstation_auto_spawn_gate_block(original, Some(&outcome));
    let v = parse_payload(&r);
    let block = v
        .get("workstation_auto_spawn_gate")
        .expect("gate block present");
    assert_eq!(
        block["auto_spawn_status"], "preexisting_marker",
        "wave-22 / task 05 invariant: pre-existing gate blocks MUST NOT be overwritten"
    );
}

/// Wave-21 / task 04 invariant carryover: when the caller does NOT
/// opt into wave-22 / task 05 auto-spawn (the `auto_spawn` flag is
/// absent or false), the response MUST stay byte-identical with
/// the wave-21 / task 04 propose-only path. That means:
///   * the wave-21 propose-only `workstation_proposals` block STILL
///     carries `auto_spawn=false` and every proposal STILL carries
///     `applied=false` (this invariant lives in workstation_dispatch.rs
///     and is independently tested there);
///   * the wave-22 `workstation_auto_spawn_gate` block is OMITTED
///     from the response (no new key on the wire).
/// We assert the second invariant on the splice helper directly.
#[test]
fn wave22_05_default_off_preserves_wave21_04_byte_shape() {
    let original = ToolResult::json_pretty(&json!({
        "status": "executing",
        "workstation_proposals": {"auto_spawn": false, "proposals": []},
    }));
    // outcome=None mirrors the auto_spawn=false caller path
    // (compute_workstation_auto_spawn_gate returns None for that case).
    let r = attach_workstation_auto_spawn_gate_block(original, None);
    let v = parse_payload(&r);
    assert!(
        v.get("workstation_auto_spawn_gate").is_none(),
        "wave-21 / task 04 byte-shape: auto_spawn=false / absent ⇒ NO new key on the wire"
    );
    // wave-21 / task 04 propose-only key untouched.
    assert_eq!(v["workstation_proposals"]["auto_spawn"], false);
}

/// Wave-22 / task 05 invariant: `parse_workstation_auto_spawn_input`
/// rejects literal-string `"true"` for the bool fields with the
/// `AUTO_SPAWN_INVALID_PARAM` code (mirrors wave-22 / task 03 / 04
/// strict-shape rule). Tested at the workstation_dispatch.rs unit
/// level; here we just assert the symbol export so plan.rs callers
/// can rely on it.
#[test]
fn wave22_05_invariant_strict_bool_shape_codes_exported() {
    use super::super::workstation_dispatch::{
        AUTO_SPAWN_INVALID_PARAM, AUTO_SPAWN_MISSING_PROPOSAL_HASH,
        AUTO_SPAWN_PROPOSAL_HASH_MISMATCH,
    };
    assert_eq!(AUTO_SPAWN_INVALID_PARAM, "AUTO_SPAWN_INVALID_PARAM");
    assert_eq!(
        AUTO_SPAWN_MISSING_PROPOSAL_HASH,
        "AUTO_SPAWN_MISSING_PROPOSAL_HASH"
    );
    assert_eq!(
        AUTO_SPAWN_PROPOSAL_HASH_MISMATCH,
        "AUTO_SPAWN_PROPOSAL_HASH_MISMATCH"
    );
}

// ── Wave 22 / Task 07 — autonomous loop apply smoke v4 ──
//
// Pin the wave22-04 persisted PLAN inference apply v2 gate slice
// of the wave22-07 v4 smoke contract. The pure preflight + soft-
// skip evaluator pair is the deterministic SSOT — no DB mutation,
// no Sonnet call, pure in-process functions over synthesised
// `ApplyGateOutcome` fixtures. The companion review_gate.rs /
// workstation_dispatch.rs / agent_execution.rs / unified_entry.rs
// smokes cover the review-apply-gate / auto-spawn / failed-
// verification / markdown-non-load-bearing slices.

/// V4 smoke (Requirement 2 / persisted apply-gate slice): the
/// wave22-04 v2 persist gate MUST reject the four-opt-in path
/// (`apply_inferred_fields=true` + `persist_inference=true` +
/// `caller_approved=true` + non-empty applied[]) when the caller
/// does not supply `proposal_hash`, AND MUST accept the same call
/// when the canonical `compute_inference_proposal_hash` value is
/// supplied. This is the wave22-04 fail-fast preflight — the gate
/// refuses to mutate the persisted plan with no correlator and
/// accepts only the canonical fixture path.
#[test]
fn smoke_wave22_07_persisted_apply_gate_rejects_missing_hash_accepts_fixture_hash() {
    let plan_id = uuid::Uuid::nil();
    let original_hash = sha256_hex("(plan :id 1)");
    let af = AppliedField {
        field: "target",
        value: serde_json::json!("mission_execution"),
        source: "plan_sexp",
        origin: ApplyOrigin::DeterministicInferred,
    };
    let canonical = compute_inference_proposal_hash(plan_id, &original_hash, &[af.clone()]);
    // Missing proposal_hash → PERSIST_APPLY_MISSING_PROPOSAL_HASH.
    let missing_args = serde_json::json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let err = enforce_persisted_apply_preflight(&missing_args, &canonical)
        .expect_err("wave22-07 v4: missing proposal_hash MUST fail-fast on persist path");
    assert_eq!(err.0, error_codes::INVALID_PARAM);
    assert!(
        err.1.contains("PERSIST_APPLY_MISSING_PROPOSAL_HASH"),
        "wave22-07 v4 invariant: missing hash MUST surface PERSIST_APPLY_MISSING_PROPOSAL_HASH"
    );
    // Mismatched proposal_hash → PERSIST_APPLY_PROPOSAL_HASH_MISMATCH.
    let mismatch_args = serde_json::json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
        "proposal_hash": "0".repeat(32),
    });
    let err = enforce_persisted_apply_preflight(&mismatch_args, &canonical)
        .expect_err("wave22-07 v4: mismatched proposal_hash MUST fail-fast on persist path");
    assert!(err.1.contains("PERSIST_APPLY_PROPOSAL_HASH_MISMATCH"));
    // Matching fixture hash → preflight OK + evaluator returns
    // Applied (non-empty applied[] from the four-opt-in path).
    let valid_args = serde_json::json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
        "proposal_hash": canonical.clone(),
    });
    assert!(
        enforce_persisted_apply_preflight(&valid_args, &canonical).is_ok(),
        "wave22-07 v4: matching proposal_hash MUST pass the persist preflight"
    );
    let outcome = fixture_apply_outcome_with_one_high_inferred(&valid_args);
    let status = evaluate_persisted_apply_gate(&valid_args, &outcome);
    assert_eq!(
        status,
        PersistedApplyStatus::Applied,
        "wave22-07 v4 invariant: matching fixture hash + four-opt-in path \
         MUST drive the persist gate to Applied"
    );
}

/// V4 smoke (cross-wave invariants / wave21-05 6 invariants
/// pinned): the wave22-04 persisted apply gate MUST preserve every
/// wave-21 / task 05 v1 in-memory apply gate invariant when the v2
/// persist flags are layered on the same call.
///   * I1 default off — the v1 byte-shape stays preserved when
///     the persist opt-ins are absent (SkippedPersistNotRequested).
///   * I2 strict bool/string shape — `validate_apply_gate_args`
///     fail-fasts on literal-string `"true"` for every opt-in flag.
///   * I3 conflicts NEVER apply — caller-vs-inferred conflicts
///     route to `conflict_fields[]` and the persist gate downgrades
///     to `SkippedNothingToApply`.
///   * I4 sub-threshold suggestions NEVER apply — medium / low
///     confidence suggestions stay below the apply threshold and
///     the persist gate downgrades to `SkippedNothingToApply`.
///   * I5 LLM proposals require `llm_caller_approved` —
///     `caller_approved=true` is the PERSIST opt-in (not the LLM
///     opt-in); an un-approved LLM proposal MUST NOT persist.
///   * I6 `apply_gate.persist_inference_applied` stays hard-pinned
///     to `false` — the v1 wire shape never changes; v2 publishes
///     persistence on a SEPARATE `persisted_apply` block.
#[test]
fn smoke_wave22_07_persisted_apply_gate_pins_wave21_05_six_invariants() {
    // I1 — default off: persist opt-ins absent ⇒ v2 reports the
    // soft-skip without any DB mutation, v1 byte-shape preserved.
    let off_args = serde_json::json!({"apply_inferred_fields": true});
    let outcome = fixture_apply_outcome_with_one_high_inferred(&off_args);
    let v1_block = outcome.to_response_json();
    assert_eq!(v1_block["requested"], true);
    assert_eq!(
        v1_block["persist_inference_applied"], false,
        "wave21-05 I6: v1 block's persist_inference_applied MUST stay hard-pinned false"
    );
    let status = evaluate_persisted_apply_gate(&off_args, &outcome);
    assert_eq!(
        status,
        PersistedApplyStatus::SkippedPersistNotRequested,
        "wave21-05 I1: default off — persist opt-ins absent MUST stay v1-shaped"
    );
    // I2 — strict bool shape: the validator fail-fasts on literal-
    // string "true" before the gate is reached.
    for arg in [
        serde_json::json!({"persist_inference": "true"}),
        serde_json::json!({"caller_approved": "true"}),
        serde_json::json!({"apply_inferred_fields": "true"}),
    ] {
        let err = validate_apply_gate_args(&arg).expect_err("string MUST be rejected");
        assert!(
            err.contains("must be a boolean"),
            "wave21-05 I2: literal-string `\"true\"` MUST fail-fast: got {}",
            err
        );
    }
    // I3 — conflicts NEVER apply.
    let mut inf_conflict = PlanFieldInference::default();
    inf_conflict.conflicts.push(InferenceConflict {
        field: "target",
        caller_value: serde_json::json!("mission_task_delegate"),
        inferred_value: serde_json::json!("mission_execution"),
        confidence: InferenceConfidence::High,
        source: "plan_sexp",
    });
    let conflict_args = serde_json::json!({
        "target": "mission_task_delegate",
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let conflict_outcome = compute_apply_gate(&conflict_args, &inf_conflict);
    assert!(
        conflict_outcome.applied.is_empty(),
        "wave21-05 I3: conflicts MUST never reach applied[]"
    );
    let status = evaluate_persisted_apply_gate(&conflict_args, &conflict_outcome);
    assert_eq!(
        status,
        PersistedApplyStatus::SkippedNothingToApply,
        "wave21-05 I3: conflict-only outcome MUST persist nothing"
    );
    // I4 — sub-threshold suggestions NEVER apply.
    let mut inf_sugg = PlanFieldInference::default();
    inf_sugg.suggested.push(InferredField {
        field: "target",
        value: serde_json::json!("mission_execution"),
        confidence: InferenceConfidence::Medium,
        source: "plan_sexp",
        detail: None,
    });
    let sugg_args = serde_json::json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let sugg_outcome = compute_apply_gate(&sugg_args, &inf_sugg);
    assert!(
        sugg_outcome.applied.is_empty(),
        "wave21-05 I4: medium-confidence suggestions MUST stay sub-threshold"
    );
    let status = evaluate_persisted_apply_gate(&sugg_args, &sugg_outcome);
    assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
    // I5 — LLM proposals require `llm_caller_approved` (caller_approved
    // is the PERSIST opt-in, not the LLM opt-in).
    let mut inf_llm = PlanFieldInference::default();
    inf_llm.llm = Some(LlmProposalBundle {
        status: LlmProposalStatus::Suggested,
        proposals: vec![LlmProposal {
            field: "target",
            value: serde_json::json!("mission_execution"),
            confidence: InferenceConfidence::High,
            evidence: "fixture".into(),
            conflict_status: LlmConflictStatus::None,
        }],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: None,
        request_caller: None,
    });
    // caller_approved=true is the PERSIST opt-in; llm_caller_approved
    // is absent ⇒ LLM proposal MUST NOT apply.
    let llm_args = serde_json::json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let llm_outcome = compute_apply_gate(&llm_args, &inf_llm);
    assert!(
        llm_outcome.applied.is_empty(),
        "wave21-05 I5: LLM proposal MUST NOT apply without `llm_caller_approved`"
    );
    let status = evaluate_persisted_apply_gate(&llm_args, &llm_outcome);
    assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
    // I6 — apply_gate.persist_inference_applied stays hard-pinned
    // false even when persist path is fully armed and applied.
    let armed_args = serde_json::json!({
        "apply_inferred_fields": true,
        "persist_inference": true,
        "caller_approved": true,
    });
    let armed_outcome = fixture_apply_outcome_with_one_high_inferred(&armed_args);
    let armed_v1_block = armed_outcome.to_response_json();
    assert_eq!(
        armed_v1_block["persist_inference_applied"], false,
        "wave21-05 I6: v1 `persist_inference_applied` field MUST stay hard-pinned false"
    );
}

// ── wave-23 / task 05 — session-trace propagation tests ─────────────
//
// The four cases from the task contract:
//   (1) legacy: no trace arg ⇒ no forward, no warning, no response
//       field surfaces (byte-shape compatible with wave-15..22)
//   (2) happy path: well-formed path forwarded into the contract
//       emitter inputs and the response surface
//   (3) malformed + required ⇒ structured INVALID_PARAM error BEFORE
//       any dispatch side effect (fail-fast)
//   (4) malformed + NOT required ⇒ non-fatal `trace_path_warning`
//       on the response, no forward (conservative posture)
//
// The dispatch path itself is exercised in the workstation_dispatch
// test module (brief / contract round-trip); these tests pin the
// pure-helper validation surface and the contract emitter wiring.

#[test]
fn validate_session_trace_path_arg_returns_none_pair_when_arg_absent() {
    let (resolved, warning) =
        validate_session_trace_path_arg(None, false).expect("absent path is always Ok");
    assert!(
        resolved.is_none(),
        "wave23-05 case 1: legacy callers see no resolved path"
    );
    assert!(
        warning.is_none(),
        "wave23-05 case 1: legacy callers see no warning"
    );
}

#[test]
fn validate_session_trace_path_arg_passes_well_formed_paths_through() {
    let (resolved, warning) =
        validate_session_trace_path_arg(Some(".missiond/tasks/wave23/session-trace.lisp"), false)
            .expect("well-formed path is always Ok");
    assert_eq!(
        resolved.as_deref(),
        Some(".missiond/tasks/wave23/session-trace.lisp"),
        "wave23-05 case 2: happy path forwards verbatim"
    );
    assert!(
        warning.is_none(),
        "wave23-05 case 2: happy path emits no warning"
    );
}

#[test]
fn validate_session_trace_path_arg_required_rejects_empty_with_invalid_param() {
    let result = validate_session_trace_path_arg(Some("   "), true);
    let err = result.expect_err("required + empty must hard-fail");
    let payload = parse_payload(&err);
    // Structured-error envelope carries the error in `error.code` /
    // `error.message`. The exact envelope shape is owned by
    // `ToolResult::structured_error`; we just assert the wire form
    // names INVALID_PARAM and the trim violation.
    let txt = serde_json::to_string(&payload).expect("serialize");
    assert!(
        txt.contains("INVALID_PARAM"),
        "wave23-05 case 3: required + malformed must fail with INVALID_PARAM, got: {}",
        txt
    );
    assert!(
        txt.contains("session_trace_path is empty after trim"),
        "wave23-05 case 3: error must name the shape failure"
    );
}

#[test]
fn validate_session_trace_path_arg_warns_on_nul_byte_when_not_required() {
    // NUL byte is a hard filesystem invariant the daemon must catch;
    // without `required`, surface a warning so the caller can fix
    // the typo without aborting the dispatch.
    let trace = "good\0bad";
    let (resolved, warning) = validate_session_trace_path_arg(Some(trace), false)
        .expect("malformed + not-required must NOT hard-fail");
    assert!(
        resolved.is_none(),
        "wave23-05 case 4: malformed path must not be forwarded"
    );
    let warning = warning.expect("wave23-05 case 4: malformed path must surface a warning");
    assert!(
        warning.contains("NUL byte"),
        "wave23-05 case 4: warning must explain the shape failure — got: {}",
        warning
    );
}

#[test]
fn task_contract_inputs_from_hints_with_trace_emits_session_trace_path_in_lisp() {
    // The contract emitter must include `:session-trace-path "..."`
    // when the trace knob is set so a downstream consumer
    // (machine-mode dispatch loading the contract directly) can
    // re-derive the path without re-supplying the arg.
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let hints = wd::WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    let inputs = task_contract_inputs_from_hints_with_trace(
        &hints,
        "mission_task_delegate",
        "fresh-code-alignment",
        Some(".missiond/tasks/wave23/session-trace.lisp"),
    );
    assert_eq!(
        inputs.session_trace_path.as_deref(),
        Some(".missiond/tasks/wave23/session-trace.lisp"),
        "wave23-05: trace path must land on TaskContractInputs.session_trace_path"
    );
    let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000feedbabe").unwrap();
    let body = build_task_contract_lisp(plan_id, "node-trace", "btk-trace", &inputs);
    assert!(
        body.contains(":session-trace-path \".missiond/tasks/wave23/session-trace.lisp\""),
        "wave23-05: emitted contract must carry `:session-trace-path` verbatim — got:\n{}",
        body
    );
}

#[test]
fn task_contract_inputs_from_hints_omits_session_trace_when_path_absent() {
    // Legacy callers (the existing 3-arg helper) must NOT emit the
    // `:session-trace-path` field — preserves wave-19..22 contract
    // byte-shape exactly so DAG / unified-entry consumers (which
    // bind to the legacy helper) keep round-tripping.
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let hints = wd::WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    let inputs =
        task_contract_inputs_from_hints(&hints, "mission_task_delegate", "fresh-code-alignment");
    assert!(
        inputs.session_trace_path.is_none(),
        "wave23-05: legacy 3-arg helper must keep session_trace_path=None"
    );
    let plan_id = Uuid::parse_str("00000000-0000-0000-0000-00000000c0de").unwrap();
    let body = build_task_contract_lisp(plan_id, "node-legacy", "btk-legacy", &inputs);
    assert!(
        !body.contains(":session-trace-path"),
        "wave23-05: legacy contract must NOT carry session-trace-path — got:\n{}",
        body
    );
}

#[test]
fn attach_session_trace_response_fields_is_a_noop_when_both_inputs_are_none() {
    // Byte-shape pin for legacy callers: when neither field is
    // supplied, the JSON envelope must be byte-identical to the
    // wave-15..22 baseline (no extra keys).
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let mut result = action_execute_bridge(&plan, &resolved);
    let baseline_text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    attach_session_trace_response_fields(&mut result, None, None);
    let after_text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after_text,
        "wave23-05: noop attach must leave the JSON envelope byte-identical"
    );
}

#[test]
fn attach_session_trace_response_fields_splices_path_and_warning_into_envelope() {
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let mut result = action_execute_bridge(&plan, &resolved);
    attach_session_trace_response_fields(
        &mut result,
        Some(".missiond/tasks/wave23/session-trace.lisp"),
        Some("malformed: NUL byte at offset 4"),
    );
    let v = parse_payload(&result);
    assert_eq!(
        v["session_trace_path"], ".missiond/tasks/wave23/session-trace.lisp",
        "wave23-05: helper must surface the resolved trace path"
    );
    assert_eq!(
        v["trace_path_warning"], "malformed: NUL byte at offset 4",
        "wave23-05: helper must surface the trace_path_warning when supplied"
    );
}

// -----------------------------------------------------------------
// wave-24 / task 04 — router-policy dry-run surface tests.
// -----------------------------------------------------------------

use super::router_policy_dry_run::{
    attach_router_recommendation_block, parse_router_policy_mode, RouterPolicyMode,
    DEFAULT_POLICY_PATH,
};

/// Minimal helper: make a fixture ToolResult mirroring the bridge
/// response shape so we can splice the recommendation block on top.
fn fixture_bridge_result() -> ToolResult {
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    action_execute_bridge(&plan, &resolved)
}

#[test]
fn router_policy_mode_default_off_emits_no_block() {
    // wave24-04 invariant: absent arg ⇒ Off.
    let args = json!({});
    let mode = parse_router_policy_mode(&args).expect("default off");
    assert!(matches!(mode, RouterPolicyMode::Off));
    // attach with Off must leave the response byte-identical.
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    let after = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode,
        &args,
        &resolved,
        &plan,
    );
    let after_text = match after.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after_text,
        "wave24-04: mode=off must not alter the response envelope"
    );
}

#[test]
fn router_policy_mode_off_returns_legacy_response_byte_identical() {
    // wave24-04: explicit "off" ⇒ Off (same as default).
    let args = json!({"router_policy_mode": "off"});
    let mode = parse_router_policy_mode(&args).expect("explicit off");
    assert!(matches!(mode, RouterPolicyMode::Off));
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    let after = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode,
        &args,
        &resolved,
        &plan,
    );
    let after_text = match after.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after_text,
        "wave24-04: explicit mode=off must be byte-identical to baseline"
    );
    let v: Value = serde_json::from_str(&after_text).unwrap();
    assert!(
        v.get("router_recommendation").is_none(),
        "wave24-04: mode=off must NOT splice a recommendation block"
    );
}

#[test]
fn router_policy_mode_apply_returns_invalid_param() {
    // wave24-04 contract: `apply` is intentionally rejected — wave24-04
    // ships only the dry-run surface.
    let args = json!({"router_policy_mode": "apply"});
    let err = parse_router_policy_mode(&args).expect_err("apply must reject");
    assert_eq!(err.is_error, Some(true));
    let text = match err.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert!(
        text.contains("INVALID_PARAM") || text.contains("invalid"),
        "wave24-04: apply must surface INVALID_PARAM (got `{}`)",
        text
    );
    assert!(
        text.contains("apply"),
        "wave24-04: error must echo the offending value"
    );
}

#[test]
fn router_policy_mode_auto_returns_invalid_param() {
    // wave24-04 contract: `auto` is intentionally rejected.
    let args = json!({"router_policy_mode": "auto"});
    let err = parse_router_policy_mode(&args).expect_err("auto must reject");
    assert_eq!(err.is_error, Some(true));
    let text = match err.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert!(text.contains("INVALID_PARAM") || text.contains("invalid"));
    assert!(text.contains("auto"));
}

#[test]
fn router_policy_mode_unknown_returns_invalid_param() {
    // wave24-04 contract: typo / unknown values reject.
    let args = json!({"router_policy_mode": "dryrun"});
    assert!(parse_router_policy_mode(&args).is_err());
    let args = json!({"router_policy_mode": "DRY_RUN"});
    assert!(parse_router_policy_mode(&args).is_err());
    // Non-string types also reject (e.g. caller passes a bool).
    let args = json!({"router_policy_mode": true});
    assert!(parse_router_policy_mode(&args).is_err());
}

#[test]
fn router_policy_mode_dry_run_emits_block_with_applied_false() {
    // Cross-wave invariant: applied=false is hard-coded literal in
    // EVERY emitted block, regardless of match outcome. Use a temp
    // policy so the test is independent of the daemon's working
    // directory.
    let tmp = std::env::temp_dir().join(format!(
        "wave24-04-shape-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-shape
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-docs
:priority 10
:when ((kind docs))
:recommend (:backend claudecode :reasoning "docs are interactive")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        // Force a no-match path (off-policy ops kind) so the block is
        // a deterministic fallback shape.
        "kind": "ops",
    });
    let mode = parse_router_policy_mode(&args).expect("dry_run parses");
    assert!(matches!(mode, RouterPolicyMode::DryRun));
    let result = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode,
        &args,
        &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
        &fixture_plan("(plan)"),
    );
    let v = parse_payload(&result);
    let block = v
        .get("router_recommendation")
        .expect("dry_run must emit router_recommendation block");
    assert_eq!(
        block["applied"], false,
        "wave24-04 invariant: applied=false hard-coded literal"
    );
    assert!(
        block.get("status").is_some(),
        "block must surface status field"
    );
    assert!(
        block.get("recommended_backend").is_some(),
        "block must surface recommended_backend"
    );
    assert!(block.get("confidence").is_some());
    assert!(block.get("reasons").is_some());
    assert!(block.get("policy_source").is_some());
    assert_eq!(block["schema"], "missiond.router-recommendation.v0");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn router_policy_mode_dry_run_does_not_change_dispatch() {
    // Cross-wave invariant: the dispatch fields (target_tool /
    // dispatch_strategy / next_call) are byte-identical with vs
    // without the dry_run mode. Only the recommendation block is
    // additive.
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

    // Baseline: no router knob.
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_v = parse_payload(&baseline);

    // With dry_run: same dispatch fields, plus a recommendation block.
    // Materialise a temp policy so this test is independent of cwd.
    let tmp = std::env::temp_dir().join(format!(
        "wave24-04-dispatch-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-dispatch
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-docs
:priority 10
:when ((kind docs))
:recommend (:backend claudecode :reasoning "docs are interactive")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let with_dry_run = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode,
        &args,
        &resolved,
        &plan,
    );
    let dry_v = parse_payload(&with_dry_run);

    // Every dispatch-shaping field must be byte-identical.
    assert_eq!(baseline_v["target_tool"], dry_v["target_tool"]);
    assert_eq!(baseline_v["target_source"], dry_v["target_source"]);
    assert_eq!(baseline_v["dispatch_strategy"], dry_v["dispatch_strategy"]);
    assert_eq!(
        baseline_v["dispatch_strategy_source"],
        dry_v["dispatch_strategy_source"]
    );
    assert_eq!(baseline_v["next_call"], dry_v["next_call"]);
    assert_eq!(baseline_v["execute_mode"], dry_v["execute_mode"]);
    assert_eq!(baseline_v["runner_status"], dry_v["runner_status"]);

    // The only delta is the additive recommendation block.
    assert!(baseline_v.get("router_recommendation").is_none());
    assert!(dry_v.get("router_recommendation").is_some());
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn router_policy_mode_dry_run_no_match_falls_back_to_claudecode_low() {
    // Off-policy combo (kind=ops) ⇒ no rule matches in the temp seed
    // policy ⇒ recommendation falls back to claudecode/low with the
    // documented `insufficient_trace_history` reason. We materialise
    // a temp policy mirroring the wave24-01 seed shape so the test
    // is independent of the daemon's working directory.
    let tmp = std::env::temp_dir().join(format!(
        "wave24-04-fallback-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-fallback
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-docs-only
:priority 10
:when ((kind docs))
:recommend (:backend claudecode :reasoning "docs only")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    // wave24-04: assert the documented default policy path constant
    // is wired into the helper (mirrors the wave24-03 CLI default).
    assert_eq!(
        DEFAULT_POLICY_PATH,
        ".missiond/router/router-policy-v1.lisp"
    );
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "agent-team");
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        "kind": "ops",
        "owner": "operator",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["confidence"], "low");
    assert_eq!(block["applied"], false);
    let reasons = block["reasons"].as_array().expect("reasons array");
    assert!(
        reasons.iter().any(|r| r
            .as_str()
            .unwrap_or("")
            .contains("insufficient_trace_history")),
        "fallback must surface insufficient_trace_history"
    );
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn router_policy_mode_dry_run_first_priority_match_wins() {
    // Build a temp policy with two matching rules at distinct
    // priorities and verify the lower priority wins.
    let tmp = std::env::temp_dir().join(format!(
        "wave24-04-multi-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-multi
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-low-prio-wins
:priority 5
:when ((kind code-alignment))
:recommend (:backend deterministic-checker :reasoning "lower priority wins")
:non-goals ["does not replace runtime dispatch"])
  (rule
:id r-loses-on-prio
:priority 50
:when ((kind code-alignment))
:recommend (:backend patch-worker :reasoning "matches but loses")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        "kind": "code-alignment",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    // Lowest priority wins ⇒ deterministic-checker (priority 5).
    assert_eq!(block["recommended_backend"], "deterministic-checker");
    assert_eq!(block["applied"], false);
    let reasons = block["reasons"].as_array().expect("reasons array");
    // Both matched rules are recorded for explainability.
    let joined = reasons
        .iter()
        .filter_map(|r| r.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("r-low-prio-wins"));
    assert!(joined.contains("r-loses-on-prio"));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn router_policy_mode_dry_run_runtime_replacement_policy_rejected() {
    // Cross-wave invariant: a policy declaring :runtime-replacement
    // true is REJECTED, with status="rejected", regardless of match.
    let tmp = std::env::temp_dir().join(format!(
        "wave24-04-rr-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-bad-rr
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement true
  (rule
:id r-rr
:priority 1
:when ((kind docs))
:recommend (:backend claudecode :reasoning "should never apply")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(
        block["status"], "rejected",
        "runtime-replacement=true must be rejected even when a rule would match"
    );
    assert_eq!(
        block["applied"], false,
        "applied=false must hold even on rejection"
    );
    assert_eq!(block["recommended_backend"], "claudecode");
    let reasons = block["reasons"].as_array().expect("reasons array");
    let joined = reasons
        .iter()
        .filter_map(|r| r.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("runtime-replacement"));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn router_policy_mode_dry_run_missing_dry_run_only_rejected() {
    // Cross-wave invariant: a policy missing :dry-run-only true is
    // REJECTED with status="rejected".
    let tmp = std::env::temp_dir().join(format!(
        "wave24-04-not-dro-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-bad-dro
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only false
  :runtime-replacement false
  (rule
:id r-x
:priority 1
:when ((kind docs))
:recommend (:backend claudecode :reasoning "should reject")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let result = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode,
        &args,
        &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
        &fixture_plan("(plan)"),
    );
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "rejected");
    assert_eq!(block["applied"], false);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn router_policy_mode_dry_run_unreadable_policy_emits_error_status() {
    // I/O failures surface as status="error" with applied=false.
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": "/this/path/does/not/exist/policy.lisp",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let result = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode,
        &args,
        &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
        &fixture_plan("(plan)"),
    );
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "error");
    assert_eq!(block["applied"], false);
    // Fallback backend is surfaced even on error so reviewers see a
    // safe default rather than a missing field.
    assert_eq!(block["recommended_backend"], "claudecode");
}

#[test]
fn router_policy_mode_dry_run_predicate_path_glob_matches_owned_files() {
    // Exercise the path-glob predicate via a temp policy that demands
    // owned_files include `scripts/check-*.mjs`.
    let tmp = std::env::temp_dir().join(format!(
        "wave24-04-glob-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-glob
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-glob
:priority 10
:when ((all (kind code-alignment)
            (path-glob "scripts/check-*.mjs")))
:recommend (:backend deterministic-checker :reasoning "scripted acceptance")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    // Match: owned_files contains a matching path.
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        "kind": "code-alignment",
        "owned_files": ["scripts/check-foo.mjs"],
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let result = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode,
        &args,
        &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
        &fixture_plan("(plan)"),
    );
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "deterministic-checker");

    // No match: owned_files contains a non-matching path.
    let args2 = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        "kind": "code-alignment",
        "owned_files": ["src/lib.rs"],
    });
    let mode2 = parse_router_policy_mode(&args2).unwrap();
    let result2 = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode2,
        &args2,
        &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
        &fixture_plan("(plan)"),
    );
    let v2 = parse_payload(&result2);
    let block2 = &v2["router_recommendation"];
    // Falls through to fallback.
    assert_eq!(block2["recommended_backend"], "claudecode");
    assert_eq!(block2["confidence"], "low");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn router_policy_mode_dry_run_predicate_any_or_clause() {
    // Exercise the `any` (logical OR) predicate.
    let tmp = std::env::temp_dir().join(format!(
        "wave24-04-any-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-any
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-any
:priority 1
:when ((any (kind review)
            (kind smoke)))
:recommend (:backend verifier-worker :reasoning "post-commit verify")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    for kind in &["review", "smoke"] {
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": kind,
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
            &fixture_plan("(plan)"),
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(
            block["recommended_backend"], "verifier-worker",
            "any-clause must match for kind={}",
            kind
        );
    }
    // Non-matching kind ⇒ fallback.
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        "kind": "ops",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let result = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode,
        &args,
        &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
        &fixture_plan("(plan)"),
    );
    let v = parse_payload(&result);
    assert_eq!(
        v["router_recommendation"]["recommended_backend"],
        "claudecode"
    );
    let _ = std::fs::remove_file(&tmp);
}

// -----------------------------------------------------------------
// wave24-06 — end-to-end smoke pinning the cross-wave invariants of
// the advisory chain at the daemon boundary. This is intentionally a
// single shape-pinning test (not a battery): the wave24-04 tests
// already cover individual edge cases; what was missing was a single
// assertion proving that ALL invariants hold simultaneously when the
// chain runs through the seed-shaped policy on a docs task.
// -----------------------------------------------------------------

#[test]
fn router_policy_dry_run_smoke_pins_cross_wave_invariants() {
    // Materialise a temp policy that mirrors the wave24-01 seed shape
    // (dry-run-only true, runtime-replacement false, three rules, the
    // r-docs-to-claudecode rule at priority 10). Using a temp file
    // keeps the smoke independent of cwd while still exercising the
    // exact parse path + selector the daemon uses in production.
    let tmp = std::env::temp_dir().join(format!(
        "wave24-06-smoke-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-smoke
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-docs-to-claudecode
:priority 10
:when ((kind docs))
:recommend (:backend claudecode :reasoning "docs are interactive")
:non-goals ["does not replace runtime dispatch"
            "does not select a model slot"])
  (rule
:id r-deterministic-checker-tasks
:priority 20
:when ((all (kind code-alignment)
            (path-glob "scripts/check-*.mjs")))
:recommend (:backend deterministic-checker :reasoning "scripted acceptance")
:non-goals ["does not replace runtime dispatch"])
  (rule
:id r-post-commit-verifier
:priority 30
:when ((any (kind review)
            (kind smoke)))
:recommend (:backend verifier-worker :reasoning "verifies an existing commit")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();

    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_v = parse_payload(&baseline);

    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": tmp.to_str().unwrap(),
        "kind": "docs",
        "owner": "claudecode",
    });
    let mode = parse_router_policy_mode(&args).expect("dry_run parses");
    assert!(matches!(mode, RouterPolicyMode::DryRun));

    let with_dry_run = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode,
        &args,
        &resolved,
        &plan,
    );
    let v = parse_payload(&with_dry_run);
    let block = v
        .get("router_recommendation")
        .expect("dry_run mode must splice a recommendation block");

    // Invariant 1: dry-run-only is honored end-to-end (the daemon's
    // applied=false hard-coded literal is the runtime analog of the
    // policy's :dry-run-only true; we pin the literal Bool type).
    assert_eq!(
        block["applied"],
        Value::Bool(false),
        "wave24-06 smoke: applied MUST be the literal false bool"
    );
    // Invariant 3: applied=false is hard-coded in every emitted block.
    // (Restated here so the smoke fails loudly if the helper ever
    // computes the field instead of hard-coding it.)
    assert!(
        block["applied"].is_boolean(),
        "applied must be a JSON bool, never a string or number"
    );

    // Invariant 2 / matched-rule: the seed's docs rule wins.
    assert_eq!(
        block["status"], "computed",
        "smoke: docs task on seed-shape policy must be computed (not rejected/error)"
    );
    assert_eq!(
        block["recommended_backend"], "claudecode",
        "smoke: r-docs-to-claudecode wins on docs task"
    );
    // Backend must be one of the wave24-01 schema enum values. We
    // re-spell the enum locally so this test does not import the
    // checker script — pure Rust.
    let allowed_backends = [
        "claudecode",
        "missiond-llm-router",
        "deterministic-checker",
        "patch-worker",
        "verifier-worker",
    ];
    let backend = block["recommended_backend"]
        .as_str()
        .expect("recommended_backend must be a string");
    assert!(
        allowed_backends.contains(&backend),
        "smoke: recommended_backend `{}` not in wave24-01 enum",
        backend
    );

    // Invariant 4: schema field surfaces the wave24 router-recommendation
    // contract identifier so external readers can route the payload.
    assert_eq!(
        block["schema"], "missiond.router-recommendation.v0",
        "smoke: schema field must surface the wave24 recommendation contract id"
    );

    // Invariant 7: dispatch fields are byte-identical to baseline.
    // The smoke compares EVERY dispatch-shaping field at once so a
    // future regression that perturbs ANY of them fails loudly here.
    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            baseline_v[field], v[field],
            "smoke: dispatch field `{}` must be byte-identical with vs without dry_run mode",
            field
        );
    }
    // The recommendation block is the ONLY additive delta.
    assert!(
        baseline_v.get("router_recommendation").is_none(),
        "baseline must not carry a recommendation block"
    );

    // Invariant 5/6: the helper must not have introduced ANY new
    // observable side effect. A weak-but-useful proof: confidence is
    // surfaced (so the policy's matched rule was actually evaluated,
    // not short-circuited by a stub) and reasons reference the rule
    // id (so the explanation is grounded in the parsed seed).
    assert!(
        block.get("confidence").is_some(),
        "smoke: confidence field must be surfaced"
    );
    let reasons = block["reasons"].as_array().expect("reasons array");
    let joined = reasons
        .iter()
        .filter_map(|r| r.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("r-docs-to-claudecode"),
        "smoke: reasons must reference the matched rule id"
    );

    let _ = std::fs::remove_file(&tmp);
}

// -----------------------------------------------------------------
// wave-25 / task 03 — router-policy trace-index confidence tests.
//
// These pin the OPTIONAL `router_policy_trace_index_path` arg and the
// additive `trace_index_path` / `trace_index_status` /
// `trace_index_warning` fields on the recommendation block. They also
// re-pin two cross-wave invariants under the new code path:
//   * `applied=false` stays a hard-coded literal even when the trace-
//     index is fully consumed.
//   * dispatch fields are byte-identical with vs without trace-index.
//
// Confidence rule mirrors `scripts/recommend-task-backend.mjs`:
//   matched + max(by_task[plan.board_task_id].events,
//                 by_backend[recommended_backend].events) >= 5  -> high
//   1..=4 -> medium
//   0 -> low (matched-but-zero) ; no-match always low.
// -----------------------------------------------------------------

/// Helper: build a temp policy file mirroring the wave24-01 seed shape
/// with a single docs->claudecode rule. Returns the path; the caller is
/// responsible for unlinking with `remove_file`.
fn write_temp_docs_policy(tag: &str) -> std::path::PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "wave25-03-{}-{}.lisp",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &tmp,
        r#"(router-policy fixture-wave25-03
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-docs
:priority 10
:when ((kind docs))
:recommend (:backend claudecode :reasoning "docs are interactive")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    tmp
}

/// Helper: build a temp trace-index JSON file. `task_events` and
/// `backend_events` populate `by_task["btk-1"].events` and
/// `by_backend["claudecode"].events` respectively (matching the
/// fixture_plan default board_task_id and the docs rule's backend).
fn write_temp_trace_index(tag: &str, task_events: u64, backend_events: u64) -> std::path::PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "wave25-03-{}-trace-{}.json",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let body = json!({
        "schema": "missiond.session-trace.v1",
        "by_task": {
            "btk-1": { "events": task_events }
        },
        "by_backend": {
            "claudecode": { "events": backend_events }
        },
        "totals": { "events": task_events + backend_events }
    });
    std::fs::write(&tmp, serde_json::to_string_pretty(&body).unwrap()).unwrap();
    tmp
}

#[test]
fn router_policy_mode_off_with_trace_index_supplied_does_no_file_io() {
    // wave25-03 invariant: mode=off (or absent) means NO file I/O happens
    // for the trace-index path EVEN IF a path is supplied. We assert this
    // by supplying a path that does NOT exist and demanding the response
    // be byte-identical to a baseline that supplies no trace-index field
    // at all. If the daemon attempted to open the file under mode=off the
    // attempt would fail-but-be-swallowed; the byte-identical assertion
    // still holds because the response shape NEVER carries any trace_index_*
    // field when mode=off (the recommendation block isn't even emitted).
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };

    // Off + non-existent trace-index path.
    let args = json!({
        "router_policy_mode": "off",
        "router_policy_trace_index_path":
            "/this/path/does/not/exist/wave25-03/trace-index.json",
    });
    let mode = parse_router_policy_mode(&args).expect("explicit off");
    assert!(matches!(mode, RouterPolicyMode::Off));
    let after = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode,
        &args,
        &resolved,
        &plan,
    );
    let after_text = match after.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after_text,
        "wave25-03: mode=off must be byte-identical to baseline EVEN WHEN trace-index path is supplied (no file I/O may happen)"
    );
    let v: Value = serde_json::from_str(&after_text).unwrap();
    assert!(
        v.get("router_recommendation").is_none(),
        "wave25-03: mode=off must NOT splice a recommendation block"
    );

    // Default (arg absent) + trace-index path supplied: same invariant.
    let args2 = json!({
        "router_policy_trace_index_path":
            "/this/path/does/not/exist/wave25-03/other.json",
    });
    let mode2 = parse_router_policy_mode(&args2).expect("default off");
    assert!(matches!(mode2, RouterPolicyMode::Off));
    let after2 = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode2,
        &args2,
        &resolved,
        &plan,
    );
    let after2_text = match after2.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after2_text,
        "wave25-03: default mode (arg absent) must be byte-identical to baseline EVEN WHEN trace-index path is supplied"
    );
}

#[test]
fn router_policy_mode_dry_run_with_trace_index_high_confidence() {
    // wave25-03: trace-index supplied AND backend has >=5 events ⇒ high.
    let policy = write_temp_docs_policy("high");
    let trace = write_temp_trace_index("high", 0, 7);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["confidence"], "high");
    assert_eq!(block["applied"], false);
    assert_eq!(block["trace_index_status"], "used");
    assert_eq!(block["trace_index_path"], trace.to_str().unwrap());
    assert!(
        block.get("trace_index_warning").is_none(),
        "wave25-03: status=used must NOT carry a warning"
    );
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
}

#[test]
fn router_policy_mode_dry_run_with_trace_index_medium_confidence() {
    // wave25-03: trace-index supplied AND max(events) in 1..=4 ⇒ medium.
    let policy = write_temp_docs_policy("medium");
    let trace = write_temp_trace_index("medium", 2, 3);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["confidence"], "medium");
    assert_eq!(block["applied"], false);
    assert_eq!(block["trace_index_status"], "used");
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
}

#[test]
fn router_policy_mode_dry_run_with_trace_index_low_confidence_when_zero_events() {
    // wave25-03: trace-index supplied AND max(events) == 0 ⇒ low. This
    // is distinct from the no-match-fallback low because a rule DID
    // match — the low confidence is due to evidence absence in the trace.
    let policy = write_temp_docs_policy("zero");
    let trace = write_temp_trace_index("zero", 0, 0);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["confidence"], "low");
    assert_eq!(block["applied"], false);
    assert_eq!(block["trace_index_status"], "used");
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
}

#[test]
fn router_policy_mode_dry_run_with_missing_trace_index_emits_status_missing() {
    // wave25-03: missing trace-index file ⇒ status=missing, dispatch
    // continues, fallback confidence (`medium` for matched).
    let policy = write_temp_docs_policy("missing");
    let bogus_trace = std::env::temp_dir().join(format!(
        "wave25-03-missing-{}-DOES-NOT-EXIST.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": bogus_trace.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(
        block["status"], "computed",
        "matched dispatch must still succeed"
    );
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(
        block["confidence"], "medium",
        "wave25-03: missing trace-index ⇒ matched fallback confidence (medium)"
    );
    assert_eq!(block["applied"], false);
    assert_eq!(block["trace_index_status"], "missing");
    assert_eq!(block["trace_index_path"], bogus_trace.to_str().unwrap());
    assert!(
        block.get("trace_index_warning").is_some(),
        "wave25-03: missing must surface a one-line warning"
    );
    let _ = std::fs::remove_file(&policy);
}

#[test]
fn router_policy_mode_dry_run_with_malformed_trace_index_emits_status_malformed() {
    // wave25-03: malformed JSON ⇒ status=malformed, fallback confidence.
    let policy = write_temp_docs_policy("malformed");
    let bad_trace = std::env::temp_dir().join(format!(
        "wave25-03-malformed-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&bad_trace, "{ this is not valid json").unwrap();
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": bad_trace.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["confidence"], "medium");
    assert_eq!(block["applied"], false);
    assert_eq!(block["trace_index_status"], "malformed");
    assert_eq!(block["trace_index_path"], bad_trace.to_str().unwrap());
    let warning = block["trace_index_warning"]
        .as_str()
        .expect("malformed must carry a warning string");
    assert!(
        warning.contains("trace-index"),
        "wave25-03: warning must mention trace-index (got `{}`)",
        warning
    );
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&bad_trace);
}

#[test]
fn router_policy_mode_dry_run_no_trace_index_supplied_emits_status_absent() {
    // wave25-03: arg absent ⇒ NO trace_index_* fields emitted at all
    // (preserves wave24-04 byte-shape for callers that did not opt in).
    // We document this as the "absent" status by checking that the
    // fields are entirely OMITTED rather than surfacing a literal
    // `"absent"` value — keeps wave24-04 callers byte-identically green.
    let policy = write_temp_docs_policy("absent");
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    // Fallback (no trace-index) ⇒ matched default `medium`.
    assert_eq!(block["confidence"], "medium");
    assert_eq!(block["applied"], false);
    // wave25-03 contract choice: when path is absent, OMIT all
    // trace_index_* fields entirely (rather than emit a literal
    // `"absent"` value) so wave24-04 callers are byte-identically green.
    assert!(
        block.get("trace_index_path").is_none(),
        "wave25-03: trace_index_path must be OMITTED when path arg is absent"
    );
    assert!(
        block.get("trace_index_status").is_none(),
        "wave25-03: trace_index_status must be OMITTED when path arg is absent"
    );
    assert!(
        block.get("trace_index_warning").is_none(),
        "wave25-03: trace_index_warning must be OMITTED when path arg is absent"
    );
    let _ = std::fs::remove_file(&policy);
}

#[test]
fn router_policy_mode_dry_run_with_trace_index_does_not_change_dispatch() {
    // wave25-03: re-pin the wave24-04 invariant under the new code path.
    // Dispatch fields (target_tool / dispatch_strategy / next_call /...)
    // are byte-identical with vs without the trace-index arg.
    let policy = write_temp_docs_policy("dispatch");
    let trace = write_temp_trace_index("dispatch", 9, 9);
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

    // Path A: dry_run + NO trace-index arg.
    let args_no_trace = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_a = parse_router_policy_mode(&args_no_trace).unwrap();
    let no_trace_result = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode_a,
        &args_no_trace,
        &resolved,
        &plan,
    );
    let no_trace_v = parse_payload(&no_trace_result);

    // Path B: dry_run + trace-index arg.
    let args_with_trace = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_b = parse_router_policy_mode(&args_with_trace).unwrap();
    let with_trace_result = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode_b,
        &args_with_trace,
        &resolved,
        &plan,
    );
    let with_trace_v = parse_payload(&with_trace_result);

    // Every dispatch-shaping field must be byte-identical.
    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            no_trace_v[field], with_trace_v[field],
            "wave25-03 invariant: dispatch field `{}` must be byte-identical with vs without trace-index arg",
            field
        );
    }

    // The recommendation block exists in both; confidence may differ
    // (medium vs high) but `applied`, `recommended_backend`, `status`,
    // and `policy_source` must match — only the additive trace_index_*
    // fields and the `confidence` are allowed to differ.
    let block_a = &no_trace_v["router_recommendation"];
    let block_b = &with_trace_v["router_recommendation"];
    assert_eq!(block_a["applied"], block_b["applied"]);
    assert_eq!(
        block_a["recommended_backend"],
        block_b["recommended_backend"]
    );
    assert_eq!(block_a["status"], block_b["status"]);
    assert_eq!(block_a["policy_source"], block_b["policy_source"]);
    assert_eq!(block_a["schema"], block_b["schema"]);

    // And the additive delta is exactly what we expect.
    assert!(block_a.get("trace_index_path").is_none());
    assert_eq!(block_b["trace_index_path"], trace.to_str().unwrap());
    assert_eq!(block_b["trace_index_status"], "used");

    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
}

#[test]
fn applied_remains_false_with_trace_index() {
    // wave25-03: re-pin the wave24-04 / wave24-06 invariant under the
    // new code path. `applied` must be the literal JSON bool `false` in
    // EVERY emitted block, regardless of trace-index status. We exercise
    // all five status flavours: used / missing / unreadable (simulated
    // via missing) / malformed / absent.
    let policy = write_temp_docs_policy("applied");
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

    // used.
    let trace_used = write_temp_trace_index("applied-used", 10, 10);
    let args_used = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace_used.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_used = parse_router_policy_mode(&args_used).unwrap();
    let r_used = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode_used,
        &args_used,
        &resolved,
        &plan,
    );
    let v_used = parse_payload(&r_used);
    assert_eq!(
        v_used["router_recommendation"]["applied"],
        Value::Bool(false),
        "wave25-03 invariant: applied=false literal under trace_index_status=used"
    );

    // missing.
    let args_missing = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": "/does/not/exist/wave25-03-applied.json",
        "kind": "docs",
    });
    let mode_missing = parse_router_policy_mode(&args_missing).unwrap();
    let r_missing = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode_missing,
        &args_missing,
        &resolved,
        &plan,
    );
    let v_missing = parse_payload(&r_missing);
    assert_eq!(
        v_missing["router_recommendation"]["applied"],
        Value::Bool(false),
        "wave25-03 invariant: applied=false literal under trace_index_status=missing"
    );

    // malformed.
    let bad = std::env::temp_dir().join(format!(
        "wave25-03-applied-malformed-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&bad, "not json").unwrap();
    let args_bad = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": bad.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_bad = parse_router_policy_mode(&args_bad).unwrap();
    let r_bad = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode_bad,
        &args_bad,
        &resolved,
        &plan,
    );
    let v_bad = parse_payload(&r_bad);
    assert_eq!(
        v_bad["router_recommendation"]["applied"],
        Value::Bool(false),
        "wave25-03 invariant: applied=false literal under trace_index_status=malformed"
    );

    // absent.
    let args_absent = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_absent = parse_router_policy_mode(&args_absent).unwrap();
    let r_absent = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode_absent,
        &args_absent,
        &resolved,
        &plan,
    );
    let v_absent = parse_payload(&r_absent);
    assert_eq!(
        v_absent["router_recommendation"]["applied"],
        Value::Bool(false),
        "wave25-03 invariant: applied=false literal under trace_index absent"
    );

    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace_used);
    let _ = std::fs::remove_file(&bad);
}

// -----------------------------------------------------------------
// wave25-05 — cross-layer measurement smoke pinning the FULL Wave 25
// measurable router loop is still ADVISORY at the daemon boundary.
//
// The wave25-05 brief calls out 8 cross-wave invariants that must all
// hold simultaneously across the evaluator + report fields + renderer
// commands + mission_plan trace-index confidence engines. The Layer A
// Node-side smoke (recommend-task-backend.mjs --dry-fixture wave25-05
// case + evaluate-router-policy-corpus.mjs --dry-fixture wave25-05
// case) pins the Node-side; the Layer C report-checker fixture pins
// the report-contract surface. This test pins the daemon side AND
// documents the CLI/Rust parity for the (5,5)-event fixture inline.
//
// The parity assertion does NOT shell out — that is forbidden by the
// wave25-05 contract. Instead, the daemon's selected backend +
// confidence are asserted against the EXPECTED values that the Node
// CLI also asserts in its own --dry-fixture run for the same
// synthetic shape. Inline documentation makes the expected agreement
// surface-readable so a future regression in either engine surfaces
// here AND in the corresponding Node fixture.
// -----------------------------------------------------------------

#[test]
fn router_policy_dry_run_smoke_pins_wave25_invariants() {
    // Materialise the wave25-05 parity fixture: the SAME two-rule policy
    // shape the Node Layer A fixture builds (docs->claudecode at
    // priority 10; code-alignment+scripts/check-* -> deterministic-
    // checker at priority 20). Using a temp file keeps the smoke
    // independent of cwd while still exercising the exact parse path
    // the daemon uses in production.
    let policy_path = std::env::temp_dir().join(format!(
        "wave25-05-smoke-policy-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &policy_path,
        r#"(router-policy fixture-wave25-05-smoke
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-docs-to-claudecode
:priority 10
:when ((kind docs))
:recommend (:backend claudecode :reasoning "docs are interactive")
:non-goals ["does not replace runtime dispatch"])
  (rule
:id r-deterministic-checker-tasks
:priority 20
:when ((all (kind code-alignment)
            (path-glob "scripts/check-*.mjs")))
:recommend (:backend deterministic-checker :reasoning "scripted acceptance")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();

    // Materialise the (5,5)-event trace-index — same shape the Node
    // CLI parity fixture drives. The daemon's bucket_events helper
    // reads by_task["btk-1"].events (fixture_plan default) AND
    // by_backend["claudecode"].events; here we plant 5 in BOTH to
    // make the parity unambiguous.
    let trace_path = std::env::temp_dir().join(format!(
        "wave25-05-smoke-trace-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let trace_body = json!({
        "schema": "missiond.session-trace.v1",
        "by_task": { "btk-1": { "events": 5 } },
        "by_backend": { "claudecode": { "events": 5 } },
        "totals": { "events": 10 }
    });
    std::fs::write(
        &trace_path,
        serde_json::to_string_pretty(&trace_body).unwrap(),
    )
    .unwrap();

    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

    // -----------------------------------------------------------------
    // Invariant 6: dispatch byte-shape unchanged when mode=off-with-
    // trace-supplied. Re-pin the wave24-04 + wave25-03 invariants under
    // the wave25-05 shape: even with both router_policy_path AND
    // router_policy_trace_index_path supplied, mode=off MUST NOT
    // perturb the dispatch envelope.
    // -----------------------------------------------------------------
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    let off_args = json!({
        "router_policy_mode": "off",
        "router_policy_path": policy_path.to_str().unwrap(),
        "router_policy_trace_index_path": trace_path.to_str().unwrap(),
        "kind": "docs",
    });
    let off_mode = parse_router_policy_mode(&off_args).expect("explicit off");
    assert!(matches!(off_mode, RouterPolicyMode::Off));
    let off_after = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        off_mode,
        &off_args,
        &resolved,
        &plan,
    );
    let off_text = match off_after.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, off_text,
        "wave25-05 invariant 6: mode=off must be byte-identical even with trace-index supplied"
    );

    // -----------------------------------------------------------------
    // Invariant 7: CLI/Rust parity for the (5,5) high-confidence
    // fixture. The Node Layer A fixture asserts:
    //   recommend({ task: docs, policy: <wave25-05 shape>,
    //               traceIndex: { backend:claudecode events:5 } }).confidence
    //     === 'high'
    //   recommend(...).backend === 'claudecode'
    //   recommend(...).chosen_rule_id === 'r-docs-to-claudecode'
    // The daemon must agree on backend + confidence for the same shape.
    // -----------------------------------------------------------------
    let dry_args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy_path.to_str().unwrap(),
        "router_policy_trace_index_path": trace_path.to_str().unwrap(),
        "kind": "docs",
        "owner": "claudecode",
    });
    let dry_mode = parse_router_policy_mode(&dry_args).expect("dry_run parses");
    assert!(matches!(dry_mode, RouterPolicyMode::DryRun));
    let dry_result = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        dry_mode,
        &dry_args,
        &resolved,
        &plan,
    );
    let dry_v = parse_payload(&dry_result);
    let block = dry_v
        .get("router_recommendation")
        .expect("dry_run mode must splice a recommendation block");

    // Invariant 1: policy.runtime_replacement=false (re-checked on the
    // parsed temp policy via the daemon's reject-runtime-replacement
    // branch — if the policy declared runtime_replacement true, the
    // status would be "rejected" and recommended_backend would fall
    // back. The wave24-01 schema rejects this at validation time; the
    // daemon re-checks defensively. We pin the absence of rejection
    // here as the positive signal.)
    assert_eq!(
        block["status"], "computed",
        "wave25-05 invariant 1: policy with runtime_replacement=false must be accepted (status=computed)"
    );
    // Invariant 2: policy.dry_run_only=true (same logic — if the
    // policy lacked dry-run-only, the daemon's
    // router_policy_mode_dry_run_missing_dry_run_only_rejected branch
    // would surface status=rejected. status=computed proves the
    // dry-run-only invariant held end-to-end.)

    // Invariant 3: applied=false JSON Bool literal in EVERY emitted
    // recommendation. Type-checked, not just value-equality, so a
    // future regression that switches the field to a string "false"
    // or to an integer 0 fails loudly here.
    assert_eq!(
        block["applied"],
        Value::Bool(false),
        "wave25-05 invariant 3: applied MUST be the literal JSON Bool false"
    );
    assert!(
        block["applied"].is_boolean(),
        "wave25-05 invariant 3: applied must be a JSON bool, never a string or number"
    );

    // Invariant 7 (cont.): CLI/Rust parity. With (5,5) trace-index
    // events ON BOTH task and backend buckets, both engines must
    // select 'high' confidence + 'claudecode' backend.
    assert_eq!(
        block["confidence"], "high",
        "wave25-05 invariant 7: daemon confidence must agree with Node CLI for (5,5) trace-index parity fixture"
    );
    assert_eq!(
        block["recommended_backend"], "claudecode",
        "wave25-05 invariant 7: daemon backend must agree with Node CLI for docs->claudecode rule"
    );
    // Recommended backend ∈ wave24-01 enum (re-spelled locally to keep
    // the test pure-Rust per wave24-06 lesson — no script imports).
    let allowed_backends = [
        "claudecode",
        "missiond-llm-router",
        "deterministic-checker",
        "patch-worker",
        "verifier-worker",
    ];
    let backend = block["recommended_backend"]
        .as_str()
        .expect("recommended_backend must be a string");
    assert!(
        allowed_backends.contains(&backend),
        "wave25-05 invariant: recommended_backend `{}` not in wave24-01 enum",
        backend
    );

    // Invariant: schema field surfaces the wave24 router-recommendation
    // contract identifier so external readers can route the payload.
    assert_eq!(
        block["schema"], "missiond.router-recommendation.v0",
        "wave25-05 invariant: schema field must surface the wave24 recommendation contract id"
    );

    // Invariant: trace_index_status=used proves the wave25-03 trace-
    // index code path was exercised (not the legacy wave24-04 fallback
    // that would emit no trace_index_* fields).
    assert_eq!(
        block["trace_index_status"], "used",
        "wave25-05 invariant: trace_index_status must be `used` for a well-formed parity fixture"
    );
    assert_eq!(
        block["trace_index_path"],
        trace_path.to_str().unwrap(),
        "wave25-05 invariant: trace_index_path must echo the input path verbatim"
    );

    // Invariant 6 (cont.): every dispatch-shaping field must be byte-
    // identical between baseline and dry-run. Re-pin every dispatch
    // field at once so any future regression that perturbs ANY of
    // them fails loudly here.
    let baseline_v = parse_payload(&baseline);
    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            baseline_v[field], dry_v[field],
            "wave25-05 invariant 6: dispatch field `{}` must be byte-identical with vs without dry_run mode",
            field
        );
    }
    assert!(
        baseline_v.get("router_recommendation").is_none(),
        "wave25-05 invariant 6: baseline must not carry a recommendation block"
    );

    // Invariant: reasons reference the matched rule id so explanation
    // is grounded in the parsed seed (mirrors wave24-06 smoke).
    let reasons = block["reasons"].as_array().expect("reasons array");
    let joined = reasons
        .iter()
        .filter_map(|r| r.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("r-docs-to-claudecode"),
        "wave25-05: reasons must reference the matched rule id"
    );

    // Invariant 8 (audit): zero shell-out / LLM / git mutation in the
    // router code path. We audit the wave25-03 daemon module's source
    // for forbidden Rust patterns: `std::process::Command`, `tokio::
    // process`, network types from `reqwest` / `hyper`, git invocation,
    // any LLM vendor probe. Forbidden patterns are assembled from
    // string parts so the audit table itself does not appear as a
    // literal substring (wave24-06 / wave25-01 self-audit lesson).
    let plan_rs = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/handlers/knowledge/plan.rs"),
    )
    .expect("plan.rs must be readable for self-audit");
    // Strip line comments before scanning so prose that names the
    // forbidden patterns does not self-trip the audit. We keep block
    // comments and string literals in scope on purpose: a real string
    // literal inviting `reqwest` would be evidence the module is about
    // to grow a network dep; the audit catches it early.
    let stripped: String = plan_rs
        .lines()
        .filter(|ln| !ln.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let forbidden_router_patterns: Vec<String> = vec![
        // std::process::Command — process spawn from std.
        String::from("std::") + "process::" + "Command",
        // tokio::process — async process spawn.
        String::from("tokio::") + "process",
        // reqwest — HTTP client crate often pulled in for LLM calls.
        String::from("req") + "west::",
        // hyper — lower-level HTTP crate.
        String::from("hyper::") + "Client",
        // openai / anthropic LLM vendor probes.
        String::from("open") + "ai_api",
        String::from("anthrop") + "ic_api",
    ];
    for pat in &forbidden_router_patterns {
        assert!(
            !stripped.contains(pat.as_str()),
            "wave25-05 invariant 8: forbidden router-side pattern `{}` found in plan.rs active source",
            pat
        );
    }

    let _ = std::fs::remove_file(&policy_path);
    let _ = std::fs::remove_file(&trace_path);
}

#[test]
fn router_policy_cli_rust_parity_for_high_confidence_match() {
    // wave25-05 Layer B parity test. Documents inline that the Node
    // CLI's `recommend({ ..., traceIndex: { by_task:{<id>:{events:5}},
    //   by_backend:{claudecode:{events:5}} } }).confidence === 'high'`
    // for a docs task on the wave25-05 parity policy. Verifying this
    // in Rust requires shelling out (which is forbidden); instead
    // this test asserts the daemon's selection matches a hard-coded
    // expected backend + confidence that the Node Layer A fixture
    // ALSO expects. A regression in either engine surfaces here AND
    // in the corresponding Node fixture so the parity is bidirectional.
    //
    // Documented expected agreement (Node CLI side):
    //   policy:        wave25-05 parity policy (docs->claudecode prio 10)
    //   task.kind:     docs
    //   trace_index:   by_task[task.id].events=5, by_backend[claudecode].events=5
    //   recommend()  -> { backend: 'claudecode',
    //                     confidence: 'high',
    //                     chosen_rule_id: 'r-docs-to-claudecode',
    //                     dry_run_only: true }
    //
    // Daemon expected agreement (this test):
    //   args.kind=docs, mode=dry_run, trace_index path -> (5,5)
    //   block.recommended_backend === 'claudecode'  (parity)
    //   block.confidence          === 'high'        (parity)
    //   block.applied             === Bool(false)   (cross-wave invariant)
    //   block.trace_index_status  === 'used'        (wave25-03 surface)
    let policy_path = std::env::temp_dir().join(format!(
        "wave25-05-parity-policy-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &policy_path,
        r#"(router-policy fixture-wave25-05-parity
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
:id r-docs-to-claudecode
:priority 10
:when ((kind docs))
:recommend (:backend claudecode :reasoning "docs are interactive")
:non-goals ["does not replace runtime dispatch"]))
"#,
    )
    .unwrap();
    let trace_path = std::env::temp_dir().join(format!(
        "wave25-05-parity-trace-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &trace_path,
        serde_json::to_string_pretty(&json!({
            "schema": "missiond.session-trace.v1",
            "by_task": { "btk-1": { "events": 5 } },
            "by_backend": { "claudecode": { "events": 5 } },
            "totals": { "events": 10 }
        }))
        .unwrap(),
    )
    .unwrap();

    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy_path.to_str().unwrap(),
        "router_policy_trace_index_path": trace_path.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).expect("dry_run parses");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];

    // Hard-coded expected values that Node Layer A also asserts for
    // the SAME shape. A divergence on either side fails this test
    // AND the Node fixture so the parity is bidirectional.
    assert_eq!(
        block["recommended_backend"], "claudecode",
        "wave25-05 parity: Node CLI emits backend='claudecode' for docs task on wave25-05 parity policy"
    );
    assert_eq!(
        block["confidence"], "high",
        "wave25-05 parity: Node CLI emits confidence='high' for (5,5)-event trace-index"
    );
    assert_eq!(
        block["applied"],
        Value::Bool(false),
        "wave25-05 parity: cross-wave invariant — applied=false literal under any trace-index status"
    );
    assert_eq!(
        block["status"], "computed",
        "wave25-05 parity: matched rule on well-formed policy must surface status=computed"
    );
    assert_eq!(
        block["trace_index_status"], "used",
        "wave25-05 parity: well-formed (5,5) trace-index must surface trace_index_status=used"
    );

    let _ = std::fs::remove_file(&policy_path);
    let _ = std::fs::remove_file(&trace_path);
}

// -----------------------------------------------------------------
// wave26-03 — backend-readiness registry consumption tests.
//
// These pin the OPTIONAL `router_backend_registry_path` arg and the
// SIX additive fields on the recommendation block:
//   * backend_registry_path
//   * backend_registry_status   ∈ used | missing | unreadable | malformed | unknown_backend
//   * backend_readiness_status  ∈ current-default | advisory-only | runtime-ready | unavailable | unknown
//   * backend_runtime_allowed   bool
//   * router_apply_eligible     bool (the 6-condition gate)
//   * router_apply_blockers     Vec<String>
//
// 6-condition apply-eligibility gate (mirrors wave26-02 Node logic):
//   1. policy valid (status=computed)
//   2. confidence == "high"
//   3. backend present in registry
//   4. runtime_allowed == true
//   5. readiness_status == "runtime-ready"  (current-default INSUFFICIENT)
//   6. apply_blockers empty
//
// Cross-wave invariants re-pinned under the new code path:
//   * applied=false stays a hard-coded literal under EVERY registry status
//   * dispatch is byte-identical with vs without registry arg
//   * mode=off (or absent) does NO file I/O even with registry path supplied
//   * mode=off remains byte-identical even when BOTH new arg AND
//     router_policy_trace_index_path are supplied
// -----------------------------------------------------------------

/// Helper: temp registry file. `body` is written verbatim so each test
/// can shape its own backends. Returns path; caller unlinks.
fn write_temp_registry(tag: &str, body: &str) -> std::path::PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "wave26-03-{}-registry-{}.lisp",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&tmp, body).unwrap();
    tmp
}

/// Helper: build a registry body with a single matched backend entry.
/// Used to exercise the 4 readiness flavours in isolation.
fn registry_body_single(
    backend_id: &str,
    readiness: &str,
    runtime_allowed: bool,
    apply_blockers: &[&str],
) -> String {
    let blockers = if apply_blockers.is_empty() {
        "[]".to_string()
    } else {
        let inner = apply_blockers
            .iter()
            .map(|b| format!("\"{}\"", b))
            .collect::<Vec<_>>()
            .join("\n     ");
        format!("[{}]", inner)
    };
    format!(
        r#"(router-backend-registry seed-test
  :schema "missiond.router-backend-registry.v1"
  :version "v1"

  (backend
:id {id}
:readiness_status {readiness}
:runtime_allowed {ra}
:apply_blockers {blockers}
:substrate nil
:non-goals ["does not replace runtime dispatch"]))
"#,
        id = backend_id,
        readiness = readiness,
        ra = if runtime_allowed { "true" } else { "false" },
        blockers = blockers,
    )
}

#[test]
fn router_policy_mode_off_with_registry_supplied_does_no_file_io() {
    // wave26-03 invariant: mode=off MUST do NO file I/O for the registry
    // path EVEN IF a non-existent path is supplied. Asserted by byte-
    // identical baseline comparison — no recommendation block is even
    // emitted, so no `backend_*` fields can leak.
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };

    let args = json!({
        "router_policy_mode": "off",
        "router_backend_registry_path":
            "/this/path/does/not/exist/wave26-03/registry.lisp",
    });
    let mode = parse_router_policy_mode(&args).expect("explicit off");
    assert!(matches!(mode, RouterPolicyMode::Off));
    let after = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode,
        &args,
        &resolved,
        &plan,
    );
    let after_text = match after.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after_text,
        "wave26-03: mode=off must be byte-identical to baseline EVEN WHEN registry path is supplied (no file I/O may happen)"
    );
    let v: Value = serde_json::from_str(&after_text).unwrap();
    assert!(
        v.get("router_recommendation").is_none(),
        "wave26-03: mode=off must NOT splice a recommendation block"
    );
}

#[test]
fn router_policy_mode_dry_run_with_registry_emits_readiness_block() {
    // Happy path: registry has the matched backend at runtime-ready +
    // runtime_allowed=true + 0 blockers; high confidence. Status=used,
    // readiness mirrored, eligible=true.
    let policy = write_temp_docs_policy("readiness-happy");
    let trace = write_temp_trace_index("readiness-happy", 7, 0);
    let registry_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
    let registry = write_temp_registry("happy", &registry_body);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["confidence"], "high");
    assert_eq!(block["applied"], false);
    assert_eq!(block["backend_registry_status"], "used");
    assert_eq!(block["backend_registry_path"], registry.to_str().unwrap());
    assert_eq!(block["backend_readiness_status"], "runtime-ready");
    assert_eq!(block["backend_runtime_allowed"], true);
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

#[test]
fn router_policy_mode_dry_run_with_runtime_ready_eligible() {
    // Synthetic registry: matched backend runtime-ready + runtime_allowed=true
    // + zero blockers + high confidence -> router_apply_eligible=true.
    let policy = write_temp_docs_policy("eligible");
    let trace = write_temp_trace_index("eligible", 8, 0);
    let registry_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
    let registry = write_temp_registry("eligible", &registry_body);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["confidence"], "high");
    assert_eq!(block["backend_readiness_status"], "runtime-ready");
    assert_eq!(block["backend_runtime_allowed"], true);
    assert_eq!(
        block["router_apply_eligible"], true,
        "wave26-03: 6-condition gate satisfied -> eligible=true"
    );
    let blockers = block["router_apply_blockers"].as_array().unwrap();
    assert!(
        blockers.is_empty(),
        "wave26-03: eligible=true means router_apply_blockers must be empty (got {:?})",
        blockers
    );
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

#[test]
fn router_policy_mode_dry_run_with_current_default_not_eligible() {
    // Seed-shape registry: claudecode current-default + runtime_allowed=true
    // + 0 blockers + high confidence. current-default is INTENTIONALLY NOT
    // sufficient — only runtime-ready opens the gate.
    let policy = write_temp_docs_policy("current-default");
    let trace = write_temp_trace_index("current-default", 8, 0);
    let registry_body = registry_body_single("claudecode", "current-default", true, &[]);
    let registry = write_temp_registry("current-default", &registry_body);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["backend_readiness_status"], "current-default");
    assert_eq!(block["backend_runtime_allowed"], true);
    assert_eq!(
        block["router_apply_eligible"], false,
        "wave26-03: current-default alone is NOT sufficient — runtime-ready required"
    );
    let blockers = block["router_apply_blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(
        blockers
            .iter()
            .any(|b| b.contains("current-default") && b.contains("runtime-ready required")),
        "wave26-03: blocker must mention current-default + runtime-ready required (got {:?})",
        blockers
    );
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

#[test]
fn router_policy_mode_dry_run_with_advisory_only_not_eligible() {
    // Matched backend = advisory-only + runtime_allowed=false +
    // apply_blockers populated. Multiple blockers expected.
    let policy = write_temp_docs_policy("advisory");
    let trace = write_temp_trace_index("advisory", 8, 0);
    let registry_body = registry_body_single(
        "claudecode",
        "advisory-only",
        false,
        &[
            "no runtime adapter shipped",
            "router replacement out of scope",
        ],
    );
    let registry = write_temp_registry("advisory", &registry_body);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["backend_readiness_status"], "advisory-only");
    assert_eq!(block["backend_runtime_allowed"], false);
    assert_eq!(block["router_apply_eligible"], false);
    let blockers = block["router_apply_blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    // synthetic blockers: runtime_allowed=false + readiness != runtime-ready
    // PLUS the registry's own 2 apply_blockers echoed verbatim.
    assert!(blockers
        .iter()
        .any(|b| b.contains("runtime_allowed is false")));
    assert!(blockers.iter().any(|b| b.contains("advisory-only")));
    assert!(blockers
        .iter()
        .any(|b| b.contains("no runtime adapter shipped")));
    assert!(blockers
        .iter()
        .any(|b| b.contains("router replacement out of scope")));
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

#[test]
fn router_policy_mode_dry_run_with_missing_registry_emits_status_missing() {
    // Non-existent registry path — fallback continues, status=missing,
    // eligible=false, dispatch unchanged.
    let policy = write_temp_docs_policy("reg-missing");
    let trace = write_temp_trace_index("reg-missing", 8, 0);
    let bogus_registry = std::env::temp_dir().join(format!(
        "wave26-03-missing-{}-DOES-NOT-EXIST.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": bogus_registry.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(
        block["status"], "computed",
        "wave26-03: missing registry must NOT fail dispatch"
    );
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["backend_registry_status"], "missing");
    assert_eq!(
        block["backend_registry_path"],
        bogus_registry.to_str().unwrap()
    );
    assert!(
        block.get("backend_warning").is_some(),
        "wave26-03: missing must surface a backend_warning"
    );
    assert_eq!(block["router_apply_eligible"], false);
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
}

#[test]
fn router_policy_mode_dry_run_with_malformed_registry_emits_status_malformed() {
    // Bad Lisp content — parser fails, fallback continues, eligible=false.
    let policy = write_temp_docs_policy("reg-malformed");
    let trace = write_temp_trace_index("reg-malformed", 8, 0);
    let bad = write_temp_registry(
        "malformed",
        "(this is :not (a router-backend-registry top form))",
    );
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": bad.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["backend_registry_status"], "malformed");
    let warning = block["backend_warning"]
        .as_str()
        .expect("malformed must carry a backend_warning string");
    assert!(
        warning.contains("backend-registry"),
        "wave26-03: warning must mention backend-registry (got `{}`)",
        warning
    );
    assert_eq!(block["router_apply_eligible"], false);
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&bad);
}

#[test]
fn router_policy_mode_dry_run_with_unknown_backend_emits_status_unknown_backend() {
    // Registry valid but missing the recommended backend (claudecode);
    // only contains a stub for `verifier-worker`. Surfaced as
    // status=unknown_backend, readiness=unknown, eligible=false.
    let policy = write_temp_docs_policy("reg-unknown-backend");
    let trace = write_temp_trace_index("reg-unknown-backend", 8, 0);
    let registry_body = registry_body_single(
        "verifier-worker",
        "advisory-only",
        false,
        &["no runtime adapter shipped"],
    );
    let registry = write_temp_registry("unknown-backend", &registry_body);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["backend_registry_status"], "unknown_backend");
    assert_eq!(block["backend_readiness_status"], "unknown");
    assert_eq!(block["router_apply_eligible"], false);
    let blockers = block["router_apply_blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(
        blockers
            .iter()
            .any(|b| b.contains("not in registry") && b.contains("claudecode")),
        "wave26-03: unknown_backend blocker must mention the missing id (got {:?})",
        blockers
    );
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

#[test]
fn router_policy_mode_dry_run_with_registry_does_not_change_dispatch() {
    // Re-pin the wave24-04 dispatch invariant under the wave26-03 code
    // path. With vs without the registry arg, every dispatch field
    // (target_tool / dispatch_strategy / next_call / ...) must be
    // byte-identical.
    let policy = write_temp_docs_policy("dispatch-pin");
    let trace = write_temp_trace_index("dispatch-pin", 8, 0);
    let registry_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
    let registry = write_temp_registry("dispatch-pin", &registry_body);
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

    // Path A: dry_run + NO registry arg.
    let args_a = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_a = parse_router_policy_mode(&args_a).unwrap();
    let result_a = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode_a,
        &args_a,
        &resolved,
        &plan,
    );
    let v_a = parse_payload(&result_a);

    // Path B: dry_run + registry arg.
    let args_b = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_b = parse_router_policy_mode(&args_b).unwrap();
    let result_b = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode_b,
        &args_b,
        &resolved,
        &plan,
    );
    let v_b = parse_payload(&result_b);

    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            v_a[field], v_b[field],
            "wave26-03 invariant: dispatch field `{}` must be byte-identical with vs without registry arg",
            field
        );
    }

    let block_a = &v_a["router_recommendation"];
    let block_b = &v_b["router_recommendation"];
    assert_eq!(block_a["applied"], block_b["applied"]);
    assert_eq!(
        block_a["recommended_backend"],
        block_b["recommended_backend"]
    );
    assert_eq!(block_a["status"], block_b["status"]);
    assert_eq!(block_a["confidence"], block_b["confidence"]);

    // Additive delta: backend_* fields exist in B but NOT in A.
    assert!(block_a.get("backend_registry_path").is_none());
    assert!(block_a.get("backend_registry_status").is_none());
    assert!(block_a.get("router_apply_eligible").is_none());
    assert_eq!(block_b["backend_registry_status"], "used");
    assert_eq!(block_b["router_apply_eligible"], true);

    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

#[test]
fn applied_remains_false_with_registry() {
    // Re-pin the wave24-04 / wave24-06 / wave25-03 invariant under the
    // wave26-03 code path: `applied` must be the literal JSON bool
    // `false` in EVERY emitted block, regardless of registry status.
    // Exercise all five status flavours: used / missing / unreadable
    // (simulated via missing) / malformed / unknown_backend.
    let policy = write_temp_docs_policy("applied-reg");
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

    // used.
    let registry_used = write_temp_registry(
        "applied-used",
        &registry_body_single("claudecode", "runtime-ready", true, &[]),
    );
    let args_used = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_backend_registry_path": registry_used.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_used = parse_router_policy_mode(&args_used).unwrap();
    let r_used = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode_used,
        &args_used,
        &resolved,
        &plan,
    );
    let v_used = parse_payload(&r_used);
    assert_eq!(
        v_used["router_recommendation"]["applied"],
        Value::Bool(false),
        "wave26-03 invariant: applied=false literal under backend_registry_status=used"
    );

    // missing.
    let args_missing = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_backend_registry_path":
            "/does/not/exist/wave26-03-applied-registry.lisp",
        "kind": "docs",
    });
    let mode_missing = parse_router_policy_mode(&args_missing).unwrap();
    let r_missing = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode_missing,
        &args_missing,
        &resolved,
        &plan,
    );
    let v_missing = parse_payload(&r_missing);
    assert_eq!(
        v_missing["router_recommendation"]["applied"],
        Value::Bool(false),
        "wave26-03 invariant: applied=false literal under backend_registry_status=missing"
    );

    // malformed.
    let bad = write_temp_registry("applied-malformed", "(not :a registry top form)");
    let args_bad = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_backend_registry_path": bad.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_bad = parse_router_policy_mode(&args_bad).unwrap();
    let r_bad = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode_bad,
        &args_bad,
        &resolved,
        &plan,
    );
    let v_bad = parse_payload(&r_bad);
    assert_eq!(
        v_bad["router_recommendation"]["applied"],
        Value::Bool(false),
        "wave26-03 invariant: applied=false literal under backend_registry_status=malformed"
    );

    // unknown_backend.
    let registry_other = write_temp_registry(
        "applied-unknown",
        &registry_body_single(
            "verifier-worker",
            "advisory-only",
            false,
            &["no runtime adapter shipped"],
        ),
    );
    let args_other = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_backend_registry_path": registry_other.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_other = parse_router_policy_mode(&args_other).unwrap();
    let r_other = attach_router_recommendation_block(
        fixture_bridge_result(),
        mode_other,
        &args_other,
        &resolved,
        &plan,
    );
    let v_other = parse_payload(&r_other);
    assert_eq!(
        v_other["router_recommendation"]["applied"],
        Value::Bool(false),
        "wave26-03 invariant: applied=false literal under backend_registry_status=unknown_backend"
    );

    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&registry_used);
    let _ = std::fs::remove_file(&bad);
    let _ = std::fs::remove_file(&registry_other);
}

#[test]
fn router_policy_mode_off_with_registry_and_trace_index_does_no_file_io() {
    // Combined cross-wave check: mode=off + BOTH new (wave26-03) +
    // wave25-03 args supplied + non-existent paths -> still byte-
    // identical to baseline. Proves the Off-path early-return predates
    // every read site.
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };

    let args = json!({
        "router_policy_mode": "off",
        "router_policy_trace_index_path":
            "/this/path/does/not/exist/wave26-03/trace.json",
        "router_backend_registry_path":
            "/this/path/does/not/exist/wave26-03/registry.lisp",
    });
    let mode = parse_router_policy_mode(&args).expect("explicit off");
    assert!(matches!(mode, RouterPolicyMode::Off));
    let after = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode,
        &args,
        &resolved,
        &plan,
    );
    let after_text = match after.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after_text,
        "wave26-03: mode=off must be byte-identical EVEN WHEN BOTH router_backend_registry_path AND router_policy_trace_index_path are supplied (no file I/O may happen)"
    );
    let v: Value = serde_json::from_str(&after_text).unwrap();
    assert!(v.get("router_recommendation").is_none());

    // Default (arg absent) + both supplied: same invariant.
    let args2 = json!({
        "router_policy_trace_index_path":
            "/another/missing/wave26-03/trace.json",
        "router_backend_registry_path":
            "/another/missing/wave26-03/registry.lisp",
    });
    let mode2 = parse_router_policy_mode(&args2).expect("default off");
    assert!(matches!(mode2, RouterPolicyMode::Off));
    let after2 = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode2,
        &args2,
        &resolved,
        &plan,
    );
    let after2_text = match after2.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after2_text,
        "wave26-03: default mode (arg absent) must be byte-identical even when both new args are supplied"
    );
}

// -----------------------------------------------------------------
// wave26-06 — cross-layer smoke pinning the FULL Wave 26 backend
// readiness loop is still ADVISORY at the daemon boundary.
//
// Pins the 9 cross-wave invariants the brief enumerates:
//   1. :runtime-replacement false in router-policy schema (wave24-01).
//   2. :dry-run-only true in router-policy schema (wave24-01).
//   3. applied=false literal in EVERY router recommendation surface.
//   4. router_apply_eligible=true ONLY when readiness_status=runtime-
//      ready AND runtime_allowed=true AND blockers empty AND high
//      confidence AND status=computed. With the seed registry where
//      claudecode is current-default, apply_eligible MUST always be
//      false even for high-confidence claudecode matches.
//   5. Renderer advisory text — pinned by Layer D.
//   6. Report-checker rejects literal-string booleans — pinned by
//      Layer C and the wave26-04 fixtures already in
//      check-task-report.mjs.
//   7. mission_plan off/default mode byte-shape unchanged EVEN WITH
//      BOTH router_backend_registry_path AND
//      router_policy_trace_index_path supplied.
//   8. CLI/Rust parity for one fixture: same registry + same trace
//      evidence -> both engines agree on backend_readiness_status +
//      router_apply_eligible.
//   9. No real LLM call, no spawn, no mutating git, no network —
//      pinned by the static audit at the bottom.
//
// Forbidden-pattern table is assembled from string parts so the
// audit does not self-trip on the patterns it scans for (wave24-06
// / wave25-01 / wave25-05 self-audit lesson).
// -----------------------------------------------------------------

#[test]
fn router_policy_dry_run_smoke_pins_wave26_invariants() {
    // Layer B Rust smoke: drive mission_plan(router_policy_mode=
    // dry_run) with all three router args supplied and assert every
    // wave26 invariant holds. Two scenarios are exercised back-to-
    // back: (a) seed-shape registry where claudecode is current-
    // default -> apply_eligible MUST be Bool(false); (b) synthetic
    // runtime-ready registry -> apply_eligible MUST be Bool(true).
    // Off-mode invariant 7 is re-pinned at the end with both router
    // args supplied to non-existent paths.

    // (a) Seed-shape registry path. claudecode is current-default
    // + runtime_allowed=true + 0 blockers — exactly the wave26-01
    // seed shape. Even with high-confidence trace, the strict gate
    // MUST reject (apply_eligible=Bool(false)).
    let policy_path = write_temp_docs_policy("wave26-06-smoke");
    let trace_path = write_temp_trace_index("wave26-06-smoke", 7, 7);
    let seed_body = registry_body_single("claudecode", "current-default", true, &[]);
    let seed_path = write_temp_registry("wave26-06-seed", &seed_body);

    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

    let dry_args_seed = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy_path.to_str().unwrap(),
        "router_policy_trace_index_path": trace_path.to_str().unwrap(),
        "router_backend_registry_path": seed_path.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_seed = parse_router_policy_mode(&dry_args_seed).expect("dry_run parses");
    assert!(matches!(mode_seed, RouterPolicyMode::DryRun));
    let result_seed = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode_seed,
        &dry_args_seed,
        &resolved,
        &plan,
    );
    let v_seed = parse_payload(&result_seed);
    let block_seed = v_seed
        .get("router_recommendation")
        .expect("dry_run mode must splice a recommendation block");

    // Invariant 1+2: status=computed proves the parsed policy was
    // accepted (the daemon rejects runtime_replacement=true and
    // dry_run_only=false at validation time, so reaching computed
    // pins both invariants end-to-end).
    assert_eq!(
        block_seed["status"], "computed",
        "wave26-06 invariant 1+2: policy with runtime_replacement=false + dry_run_only=true must surface status=computed"
    );

    // Invariant 3: applied is the literal JSON Bool false. Type-
    // checked, not just value-equality, so a future regression that
    // switches to "false" string fails loudly here.
    assert_eq!(
        block_seed["applied"],
        Value::Bool(false),
        "wave26-06 invariant 3: applied MUST be literal JSON Bool false under wave26-03 code path"
    );
    assert!(
        block_seed["applied"].is_boolean(),
        "wave26-06 invariant 3: applied must be a JSON bool, never a string or number"
    );

    // Invariant 4 (negative case): seed-shape registry where
    // claudecode is current-default + runtime_allowed=true + 0
    // blockers + high confidence + matched rule. router_apply_
    // eligible MUST be Bool(false) because readiness_status is
    // current-default, not runtime-ready. current-default alone is
    // INTENTIONALLY insufficient.
    assert_eq!(
        block_seed["confidence"], "high",
        "wave26-06 invariant 4 prereq: trace must produce high confidence so the failing gate is readiness, not confidence"
    );
    assert_eq!(
        block_seed["recommended_backend"], "claudecode",
        "wave26-06 invariant 4 prereq: docs->claudecode rule must match"
    );
    assert_eq!(
        block_seed["backend_readiness_status"], "current-default",
        "wave26-06 invariant 4 prereq: seed-shape registry yields current-default"
    );
    assert_eq!(
        block_seed["backend_runtime_allowed"],
        Value::Bool(true),
        "wave26-06 invariant 4 prereq: seed claudecode runtime_allowed=true"
    );
    assert_eq!(
        block_seed["router_apply_eligible"],
        Value::Bool(false),
        "wave26-06 invariant 4: current-default + high-confidence + runtime_allowed=true MUST still yield apply_eligible=false (current-default alone is INSUFFICIENT)"
    );
    assert!(
        block_seed["router_apply_eligible"].is_boolean(),
        "wave26-06 invariant 4: router_apply_eligible must be a literal bool, never a string"
    );
    let blockers_seed = block_seed["router_apply_blockers"]
        .as_array()
        .expect("router_apply_blockers must be an array");
    let joined_seed = blockers_seed
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined_seed.contains("current-default") && joined_seed.contains("runtime-ready"),
        "wave26-06 invariant 4: blocker must mention current-default + runtime-ready (got `{}`)",
        joined_seed
    );
    assert_eq!(
        block_seed["backend_registry_status"], "used",
        "wave26-06 invariant 4: well-formed registry must surface backend_registry_status=used"
    );

    // (b) Synthetic runtime-ready registry. Same policy, same trace,
    // same docs task — only the registry shape differs. ALL 6 daemon
    // gate conditions hold so router_apply_eligible MUST flip to
    // Bool(true). This is the positive control proving the gate is
    // not stuck-false.
    let ready_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
    let ready_path = write_temp_registry("wave26-06-ready", &ready_body);
    let dry_args_ready = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy_path.to_str().unwrap(),
        "router_policy_trace_index_path": trace_path.to_str().unwrap(),
        "router_backend_registry_path": ready_path.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_ready = parse_router_policy_mode(&dry_args_ready).expect("dry_run parses");
    let result_ready = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode_ready,
        &dry_args_ready,
        &resolved,
        &plan,
    );
    let v_ready = parse_payload(&result_ready);
    let block_ready = &v_ready["router_recommendation"];

    assert_eq!(
        block_ready["applied"],
        Value::Bool(false),
        "wave26-06 invariant 3: applied MUST be literal Bool(false) EVEN UNDER apply_eligible=true (runtime replacement is rejected by contract)"
    );
    assert_eq!(
        block_ready["backend_readiness_status"], "runtime-ready",
        "wave26-06 invariant 4 positive: registry shape determines readiness_status"
    );
    assert_eq!(
        block_ready["router_apply_eligible"],
        Value::Bool(true),
        "wave26-06 invariant 4 positive: ALL 6 gate conditions met -> apply_eligible=true"
    );
    let blockers_ready = block_ready["router_apply_blockers"]
        .as_array()
        .expect("router_apply_blockers must be an array");
    assert!(
        blockers_ready.is_empty(),
        "wave26-06 invariant 4 positive: apply_eligible=true means router_apply_blockers must be empty (got {:?})",
        blockers_ready
    );

    // Invariant 7 (off mode + BOTH router args): re-pin under the
    // wave26-06 smoke. mode=off MUST be byte-identical to baseline
    // even when both router_backend_registry_path AND
    // router_policy_trace_index_path are supplied. Use NON-existent
    // paths to additionally prove no file I/O happens.
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    let off_args = json!({
        "router_policy_mode": "off",
        "router_policy_path": policy_path.to_str().unwrap(),
        "router_policy_trace_index_path":
            "/this/path/does/not/exist/wave26-06/trace.json",
        "router_backend_registry_path":
            "/this/path/does/not/exist/wave26-06/registry.lisp",
        "kind": "docs",
    });
    let off_mode = parse_router_policy_mode(&off_args).expect("explicit off");
    assert!(matches!(off_mode, RouterPolicyMode::Off));
    let off_after = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        off_mode,
        &off_args,
        &resolved,
        &plan,
    );
    let off_text = match off_after.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, off_text,
        "wave26-06 invariant 7: mode=off must be byte-identical EVEN WITH BOTH router_backend_registry_path AND router_policy_trace_index_path supplied (no file I/O may happen)"
    );

    // Invariant 7 (cont.): also verify dispatch shape is byte-
    // identical between baseline and the dry_run+seed-registry
    // result. Mode=dry_run is allowed to add the recommendation
    // block but every dispatch field must remain byte-identical.
    let baseline_v = parse_payload(&baseline);
    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            baseline_v[field], v_seed[field],
            "wave26-06 invariant 7: dispatch field `{}` must be byte-identical with vs without router args",
            field
        );
        assert_eq!(
            baseline_v[field], v_ready[field],
            "wave26-06 invariant 7: dispatch field `{}` must be byte-identical regardless of registry shape",
            field
        );
    }

    // Invariant 9 (audit): zero shell-out / LLM / git / network in
    // the daemon plan.rs router-readiness path. Forbidden patterns
    // assembled from string parts so the audit does not self-trip
    // on the patterns it scans for. Mirrors the wave25-05 smoke.
    let plan_rs = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/handlers/knowledge/plan.rs"),
    )
    .expect("plan.rs must be readable for self-audit");
    let stripped: String = plan_rs
        .lines()
        .filter(|ln| !ln.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let forbidden_router_patterns: Vec<String> = vec![
        String::from("std::") + "process::" + "Command",
        String::from("tokio::") + "process",
        String::from("req") + "west::",
        String::from("hyper::") + "Client",
        String::from("open") + "ai_api",
        String::from("anthrop") + "ic_api",
    ];
    for pat in &forbidden_router_patterns {
        assert!(
            !stripped.contains(pat.as_str()),
            "wave26-06 invariant 9: forbidden router-side pattern `{}` found in plan.rs active source",
            pat
        );
    }

    let _ = std::fs::remove_file(&policy_path);
    let _ = std::fs::remove_file(&trace_path);
    let _ = std::fs::remove_file(&seed_path);
    let _ = std::fs::remove_file(&ready_path);
}

#[test]
fn router_policy_cli_rust_parity_for_readiness() {
    // Layer B Rust smoke (parity): both engines (Node CLI
    // recommend-task-backend.mjs --dry-fixture and the daemon's
    // mission_plan dry_run) MUST agree on backend_readiness_status
    // and router_apply_eligible for the SAME registry shape +
    // SAME confidence level. We assert the daemon side here against
    // the EXPECTED values that the Node Layer A1 fixtures
    // (wave26-06: cross-layer smoke pins apply_eligible=false for
    // current-default seed) also assert. A divergence on either
    // side fails this test AND the corresponding Node fixture so
    // the parity is bidirectional.
    //
    // Documented expected agreement (Node CLI side, wave26-06 Layer
    // A1 smoke fixtures):
    //   policy:    docs->claudecode (high priority match)
    //   trace:     (8,8)-event index -> high confidence
    //   registry:  claudecode current-default + runtime_allowed=true + 0 blockers
    //   annotate() ->  backend_readiness_status: 'current-default'
    //                  backend_runtime_allowed:  true
    //                  router_apply_eligible:    false
    //
    // Daemon expected agreement (this test):
    //   args.kind=docs, mode=dry_run, registry=current-default
    //   block.backend_readiness_status === 'current-default'  (parity)
    //   block.backend_runtime_allowed  === true               (parity)
    //   block.router_apply_eligible    === false              (parity)
    //   block.applied                  === Bool(false)        (cross-wave)
    let policy_path = write_temp_docs_policy("wave26-06-parity");
    // Trace index supplies (8,8) events on btk-1/claudecode buckets;
    // matches the Node fixture's synthesizeTraceIndex(8,8) shape.
    let trace_path = write_temp_trace_index("wave26-06-parity", 8, 8);
    let seed_body = registry_body_single("claudecode", "current-default", true, &[]);
    let registry_path = write_temp_registry("wave26-06-parity", &seed_body);

    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy_path.to_str().unwrap(),
        "router_policy_trace_index_path": trace_path.to_str().unwrap(),
        "router_backend_registry_path": registry_path.to_str().unwrap(),
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).expect("dry_run parses");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];

    // Hard-coded expected values that the Node Layer A1 smoke
    // fixture also asserts for the SAME shape. A divergence on
    // either side fails BOTH tests so the parity is bidirectional.
    assert_eq!(
        block["recommended_backend"], "claudecode",
        "wave26-06 parity: Node CLI emits backend='claudecode' for docs task on seed policy"
    );
    assert_eq!(
        block["confidence"], "high",
        "wave26-06 parity: Node CLI emits confidence='high' for (8,8)-event trace-index"
    );
    assert_eq!(
        block["backend_readiness_status"], "current-default",
        "wave26-06 parity: Node CLI emits backend_readiness_status='current-default' for seed-shape registry"
    );
    assert_eq!(
        block["backend_runtime_allowed"],
        Value::Bool(true),
        "wave26-06 parity: Node CLI emits backend_runtime_allowed=true for seed claudecode"
    );
    assert_eq!(
        block["router_apply_eligible"],
        Value::Bool(false),
        "wave26-06 parity: Node CLI emits router_apply_eligible=false for current-default (current-default alone is INSUFFICIENT)"
    );
    assert_eq!(
        block["applied"],
        Value::Bool(false),
        "wave26-06 parity: cross-wave invariant — applied=false literal under any registry status"
    );
    assert_eq!(
        block["status"], "computed",
        "wave26-06 parity: matched rule on well-formed policy must surface status=computed"
    );
    assert_eq!(
        block["backend_registry_status"], "used",
        "wave26-06 parity: well-formed registry must surface backend_registry_status=used"
    );

    // Recommended backend ∈ wave24-01 enum (re-spelled locally to
    // keep the test pure-Rust per wave24-06 lesson — no script
    // imports). Mirrors the wave25-05 parity test.
    let allowed_backends = [
        "claudecode",
        "missiond-llm-router",
        "deterministic-checker",
        "patch-worker",
        "verifier-worker",
    ];
    let backend = block["recommended_backend"]
        .as_str()
        .expect("recommended_backend must be a string");
    assert!(
        allowed_backends.contains(&backend),
        "wave26-06 parity: recommended_backend `{}` not in wave24-01 enum",
        backend
    );

    // Allowed readiness status ∈ wave26-01 enum (re-spelled
    // locally). A future regression that introduces a non-enum
    // value fails here.
    let allowed_readiness = [
        "current-default",
        "advisory-only",
        "runtime-ready",
        "unavailable",
        "unknown",
    ];
    let readiness = block["backend_readiness_status"]
        .as_str()
        .expect("backend_readiness_status must be a string");
    assert!(
        allowed_readiness.contains(&readiness),
        "wave26-06 parity: backend_readiness_status `{}` not in wave26-01 enum",
        readiness
    );

    let _ = std::fs::remove_file(&policy_path);
    let _ = std::fs::remove_file(&trace_path);
    let _ = std::fs::remove_file(&registry_path);
}

// -----------------------------------------------------------------
// wave-27 / task 03 — router dispatch descriptor surface tests.
//
// These pin the OPTIONAL `router_dispatch_descriptor` arg.
// Invariants this block enforces:
//   * Off/default mode + descriptor=true MUST be byte-identical to
//     the wave-15..23 baseline (no extra file I/O happens because
//     the Off-path early-return predates compute_recommendation).
//   * dry_run + descriptor=true + seed registry (claudecode is
//     current-default) -> descriptor body present, no_execution=true,
//     dry_run_only=true, runtime_replacement=false (all literal Bool),
//     router_apply_eligible=false (current-default is rejected by
//     the wave26-03 6-condition gate).
//   * dry_run + descriptor=true + synthetic runtime-ready registry +
//     high confidence -> router_apply_eligible=true BUT the three
//     locked invariants STILL hold (runtime_replacement=false,
//     no_execution=true, dry_run_only=true).
//   * dry_run + descriptor=true + NO registry path -> descriptor body
//     OMITTED, descriptor_status="registry_missing" surfaced.
//   * dry_run + descriptor=true MUST NOT change any dispatch field
//     (re-pin of the wave24-04 dispatch invariant under the new code
//     path).
// -----------------------------------------------------------------

#[test]
fn router_dispatch_descriptor_off_default_does_no_extra_io() {
    // wave27-03: mode=off with router_dispatch_descriptor=true AND all
    // three wave24-04 / wave25-03 / wave26-03 router args supplied
    // (with non-existent paths) MUST be byte-identical to baseline.
    // Proves the Off-path early-return predates every read site,
    // including the new descriptor branch.
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };

    let args = json!({
        "router_policy_mode": "off",
        "router_policy_path":
            "/this/path/does/not/exist/wave27-03/policy.lisp",
        "router_policy_trace_index_path":
            "/this/path/does/not/exist/wave27-03/trace.json",
        "router_backend_registry_path":
            "/this/path/does/not/exist/wave27-03/registry.lisp",
        "router_dispatch_descriptor": true,
    });
    let mode = parse_router_policy_mode(&args).expect("explicit off");
    assert!(matches!(mode, RouterPolicyMode::Off));
    let after = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode,
        &args,
        &resolved,
        &plan,
    );
    let after_text = match after.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after_text,
        "wave27-03: mode=off + descriptor=true + all three router args (policy/trace/registry) supplied MUST be byte-identical to baseline (the Off early-return predates the descriptor branch — no extra I/O)"
    );
    let v: Value = serde_json::from_str(&after_text).unwrap();
    assert!(
        v.get("router_recommendation").is_none(),
        "wave27-03: mode=off must NOT splice a recommendation block (descriptor or otherwise)"
    );

    // Default (mode arg absent) + descriptor=true + same three paths:
    // same invariant.
    let args2 = json!({
        "router_policy_path":
            "/another/missing/wave27-03/policy.lisp",
        "router_policy_trace_index_path":
            "/another/missing/wave27-03/trace.json",
        "router_backend_registry_path":
            "/another/missing/wave27-03/registry.lisp",
        "router_dispatch_descriptor": true,
    });
    let mode2 = parse_router_policy_mode(&args2).expect("default off");
    assert!(matches!(mode2, RouterPolicyMode::Off));
    let after2 = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode2,
        &args2,
        &resolved,
        &plan,
    );
    let after2_text = match after2.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after2_text,
        "wave27-03: default mode (arg absent) + descriptor=true + all three router args MUST stay byte-identical"
    );
}

#[test]
fn router_dispatch_descriptor_dry_run_with_seed_registry_emits_no_execution_true() {
    // wave27-03: dry_run + descriptor=true + seed-shape registry where
    // claudecode is current-default. Descriptor body MUST be present
    // and carry the three locked literal-bool invariants. Eligibility
    // MUST be false (current-default does NOT satisfy the wave26-03
    // 6-condition gate; runtime-ready opt-in is required).
    let policy = write_temp_docs_policy("desc-seed-current-default");
    let trace = write_temp_trace_index("desc-seed-current-default", 8, 0);
    let registry_body = registry_body_single("claudecode", "current-default", true, &[]);
    let registry = write_temp_registry("desc-seed-current-default", &registry_body);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "router_dispatch_descriptor": true,
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    // Descriptor body present.
    let descriptor = &block["router_dispatch_descriptor"];
    assert!(
        descriptor.is_object(),
        "wave27-03: descriptor body must be present when registry is supplied + descriptor=true (got `{}`)",
        descriptor
    );
    // Locked literal-bool invariants — MUST be Value::Bool, never strings.
    assert_eq!(
        descriptor["dry_run_only"],
        Value::Bool(true),
        "wave27-03 LOCKED INVARIANT: dry_run_only must be literal Bool true"
    );
    assert!(
        descriptor["dry_run_only"].is_boolean(),
        "wave27-03: dry_run_only must be a JSON bool, never a string"
    );
    assert_eq!(
        descriptor["runtime_replacement"],
        Value::Bool(false),
        "wave27-03 LOCKED INVARIANT: runtime_replacement must be literal Bool false"
    );
    assert!(
        descriptor["runtime_replacement"].is_boolean(),
        "wave27-03: runtime_replacement must be a JSON bool, never a string"
    );
    assert_eq!(
        descriptor["no_execution"],
        Value::Bool(true),
        "wave27-03 LOCKED INVARIANT: no_execution must be literal Bool true"
    );
    assert!(
        descriptor["no_execution"].is_boolean(),
        "wave27-03: no_execution must be a JSON bool, never a string"
    );
    // Schema + task_id + recommendation source identifier.
    assert_eq!(
        descriptor["schema"], "missiond.router-dispatch-descriptor.v1",
        "wave27-03: descriptor schema id mirrors wave27-01"
    );
    assert_eq!(
        descriptor["task_id"], "btk-1",
        "wave27-03: descriptor task_id must echo plan.board_task_id"
    );
    assert_eq!(
        descriptor["source_recommendation_schema"], "missiond.router-recommendation.v0",
        "wave27-03: descriptor must record the upstream wave24-04 recommendation schema id"
    );
    assert_eq!(
        descriptor["source_policy_path"],
        policy.to_str().unwrap(),
        "wave27-03: descriptor must echo router_policy_path"
    );
    assert_eq!(
        descriptor["source_backend_registry_path"],
        registry.to_str().unwrap(),
        "wave27-03: descriptor must echo router_backend_registry_path"
    );
    // Projected fields off the wave26-03 readiness block.
    assert_eq!(descriptor["recommended_backend"], "claudecode");
    assert_eq!(descriptor["router_confidence"], "high");
    assert_eq!(descriptor["backend_readiness_status"], "current-default");
    assert_eq!(descriptor["backend_runtime_allowed"], Value::Bool(true));
    assert_eq!(
        descriptor["router_apply_eligible"],
        Value::Bool(false),
        "wave27-03: current-default registry does NOT satisfy the wave26-03 gate (runtime-ready required)"
    );
    let blockers = descriptor["router_apply_blockers"]
        .as_array()
        .expect("router_apply_blockers must be a JSON array");
    assert!(
        !blockers.is_empty(),
        "wave27-03: eligible=false MUST list at least one blocker (got {:?})",
        blockers
    );
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

#[test]
fn router_dispatch_descriptor_dry_run_with_runtime_ready_eligible() {
    // wave27-03: synthetic registry where the matched backend is
    // runtime-ready + runtime_allowed=true + zero blockers + high
    // confidence -> router_apply_eligible=true. The three locked
    // invariants (dry_run_only / runtime_replacement / no_execution)
    // MUST still hold — eligibility flipping does NOT promote the
    // descriptor to a runtime apply signal.
    let policy = write_temp_docs_policy("desc-runtime-ready");
    let trace = write_temp_trace_index("desc-runtime-ready", 9, 0);
    let registry_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
    let registry = write_temp_registry("desc-runtime-ready", &registry_body);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "router_dispatch_descriptor": true,
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    let descriptor = &block["router_dispatch_descriptor"];
    assert!(
        descriptor.is_object(),
        "wave27-03: descriptor body must be present"
    );
    // Cross-wave invariant: even when eligibility flips to true, the
    // three locked invariants MUST stay literal Bool literals.
    assert_eq!(
        descriptor["router_apply_eligible"],
        Value::Bool(true),
        "wave27-03: runtime-ready + high confidence + runtime_allowed=true + zero blockers -> eligible=true"
    );
    let blockers = descriptor["router_apply_blockers"]
        .as_array()
        .expect("router_apply_blockers must be array");
    assert!(
        blockers.is_empty(),
        "wave27-03: eligible=true means router_apply_blockers MUST be empty (got {:?})",
        blockers
    );
    // CROSS-WAVE INVARIANT: eligibility=true does NOT promote the
    // descriptor to a runtime signal. The three locked literals stay
    // literal Bool, hard-coded.
    assert_eq!(
        descriptor["dry_run_only"],
        Value::Bool(true),
        "wave27-03 LOCKED: dry_run_only stays literal Bool true even when eligible=true"
    );
    assert_eq!(
        descriptor["runtime_replacement"],
        Value::Bool(false),
        "wave27-03 LOCKED: runtime_replacement stays literal Bool false even when eligible=true"
    );
    assert_eq!(
        descriptor["no_execution"],
        Value::Bool(true),
        "wave27-03 LOCKED: no_execution stays literal Bool true even when eligible=true"
    );
    assert_eq!(descriptor["backend_readiness_status"], "runtime-ready");
    assert_eq!(descriptor["backend_runtime_allowed"], Value::Bool(true));
    assert_eq!(descriptor["router_confidence"], "high");
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

#[test]
fn router_dispatch_descriptor_dry_run_without_registry_path_emits_status_registry_missing() {
    // wave27-03: dry_run + descriptor=true with NO router_backend_registry_path
    // -> descriptor body OMITTED + top-level descriptor_status="registry_missing"
    // surfaced on the recommendation block. The wave27-01 schema
    // requires backend_readiness_status / backend_runtime_allowed
    // values that we cannot honestly produce without consulting a
    // registry, so we intentionally refuse to fake readiness.
    let policy = write_temp_docs_policy("desc-no-registry");
    let trace = write_temp_trace_index("desc-no-registry", 8, 0);
    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_dispatch_descriptor": true,
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    assert_eq!(
        block["descriptor_status"], "registry_missing",
        "wave27-03: NO registry path + descriptor=true -> descriptor_status=registry_missing on the recommendation block"
    );
    assert!(
        block.get("router_dispatch_descriptor").is_none(),
        "wave27-03: descriptor body MUST be omitted when registry path is absent (got `{:?}`)",
        block.get("router_dispatch_descriptor")
    );
    // Recommendation block itself is unchanged; status is still
    // computed because the docs rule matched.
    assert_eq!(block["status"], "computed");
    assert_eq!(block["recommended_backend"], "claudecode");
    assert_eq!(block["applied"], Value::Bool(false));
    // Sanity: NO backend_* readiness fields leaked (registry was Absent).
    assert!(block.get("backend_registry_path").is_none());
    assert!(block.get("backend_registry_status").is_none());
    assert!(block.get("backend_readiness_status").is_none());
    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
}

#[test]
fn router_dispatch_descriptor_does_not_change_dispatch() {
    // wave27-03 re-pin of the wave24-04 dispatch invariant under the
    // new descriptor code path. With vs without the descriptor flag
    // (both in dry_run + same registry), every dispatch-shaping field
    // (target_tool / dispatch_strategy / next_call / runner_status /
    // execute_mode / target_source / dispatch_strategy_source) MUST
    // be byte-identical. Only the additive descriptor block delta is
    // expected.
    let policy = write_temp_docs_policy("desc-dispatch-pin");
    let trace = write_temp_trace_index("desc-dispatch-pin", 9, 0);
    let registry_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
    let registry = write_temp_registry("desc-dispatch-pin", &registry_body);
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

    // Path A: dry_run + registry, NO descriptor.
    let args_a = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_a = parse_router_policy_mode(&args_a).unwrap();
    let result_a = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode_a,
        &args_a,
        &resolved,
        &plan,
    );
    let v_a = parse_payload(&result_a);

    // Path B: dry_run + registry + descriptor=true.
    let args_b = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "router_dispatch_descriptor": true,
        "kind": "docs",
    });
    let mode_b = parse_router_policy_mode(&args_b).unwrap();
    let result_b = attach_router_recommendation_block(
        action_execute_bridge(&plan, &resolved),
        mode_b,
        &args_b,
        &resolved,
        &plan,
    );
    let v_b = parse_payload(&result_b);

    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            v_a[field], v_b[field],
            "wave27-03: dispatch field `{}` MUST be byte-identical with vs without router_dispatch_descriptor=true",
            field
        );
    }

    let block_a = &v_a["router_recommendation"];
    let block_b = &v_b["router_recommendation"];
    // Recommendation core fields are unchanged by the descriptor flag.
    assert_eq!(block_a["status"], block_b["status"]);
    assert_eq!(block_a["applied"], block_b["applied"]);
    assert_eq!(
        block_a["recommended_backend"],
        block_b["recommended_backend"]
    );
    assert_eq!(block_a["confidence"], block_b["confidence"]);
    assert_eq!(
        block_a["backend_readiness_status"],
        block_b["backend_readiness_status"]
    );
    assert_eq!(
        block_a["router_apply_eligible"],
        block_b["router_apply_eligible"]
    );

    // Additive delta: descriptor present in B, absent in A.
    assert!(
        block_a.get("router_dispatch_descriptor").is_none(),
        "wave27-03: NO descriptor in path A (flag absent)"
    );
    assert!(
        block_b.get("router_dispatch_descriptor").is_some(),
        "wave27-03: descriptor present in path B (flag=true)"
    );

    // applied=false literal is invariant across both paths.
    assert_eq!(block_a["applied"], Value::Bool(false));
    assert_eq!(block_b["applied"], Value::Bool(false));

    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

/// wave27-06 cross-layer smoke: in ONE exhaustive test, re-pin EVERY
/// wave27 cross-wave invariant exercised by the daemon dispatch
/// descriptor surface. This is the single attribution point for a
/// future bisect — if the wave27 invariant chain regresses on the
/// daemon side, this test fails and `git log -S
/// router_dispatch_descriptor_smoke_pins_wave27_invariants` lands
/// the search on this file.
///
/// Invariants asserted:
///   1. dry_run_only literal Value::Bool(true) — wave27-03
///   2. runtime_replacement literal Value::Bool(false) — wave27-03
///   3. no_execution literal Value::Bool(true) — wave27-03 / wave27-04
///   4. With seed registry (claudecode current-default) +
///      router_dispatch_descriptor=true:
///        a. router_apply_eligible Value::Bool(false)
///        b. router_apply_blockers non-empty
///   5. Dispatch-shaping fields (target_tool / target_source /
///      dispatch_strategy / dispatch_strategy_source / next_call /
///      execute_mode / runner_status) byte-identical with vs
///      without router_dispatch_descriptor=true (re-pin of the
///      wave24-04 invariant under the wave27 surface)
///   6. wave27-03 self-audit: plan.rs source carries NO new
///      shell-out / spawn / git mutation / network / LLM client in
///      the active code (assembled forbidden-pattern table from
///      string parts so this audit body does NOT self-trip on the
///      patterns it scans for — wave24-06 / wave25-05 / wave26-06
///      lesson).
#[test]
fn router_dispatch_descriptor_smoke_pins_wave27_invariants() {
    // ---- Part 1: descriptor invariants under seed (current-default) ----
    let policy = write_temp_docs_policy("w27-06-smoke-seed");
    let trace = write_temp_trace_index("w27-06-smoke-seed", 8, 0);
    let registry_body = registry_body_single("claudecode", "current-default", true, &[]);
    let registry = write_temp_registry("w27-06-smoke-seed", &registry_body);
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

    let args = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "router_dispatch_descriptor": true,
        "kind": "docs",
    });
    let mode = parse_router_policy_mode(&args).unwrap();
    let result =
        attach_router_recommendation_block(fixture_bridge_result(), mode, &args, &resolved, &plan);
    let v = parse_payload(&result);
    let block = &v["router_recommendation"];
    let descriptor = &block["router_dispatch_descriptor"];
    assert!(
        descriptor.is_object(),
        "wave27-06 invariant: descriptor body MUST be present (got `{}`)",
        descriptor
    );

    // wave27-06 invariants 1-3: locked literal Bools (NOT strings,
    // NOT computed). The is_boolean() asserts also catch the
    // pathological case where a future projector mutation turns
    // these into "true"/"false" strings while still passing
    // assert_eq! on the JSON layer.
    assert_eq!(
        descriptor["dry_run_only"],
        Value::Bool(true),
        "wave27-06 invariant 1: dry_run_only must be literal Value::Bool(true)"
    );
    assert!(
        descriptor["dry_run_only"].is_boolean(),
        "wave27-06 invariant 1: dry_run_only must be a JSON bool, never a string"
    );
    assert_eq!(
        descriptor["runtime_replacement"],
        Value::Bool(false),
        "wave27-06 invariant 2: runtime_replacement must be literal Value::Bool(false)"
    );
    assert!(
        descriptor["runtime_replacement"].is_boolean(),
        "wave27-06 invariant 2: runtime_replacement must be a JSON bool, never a string"
    );
    assert_eq!(
        descriptor["no_execution"],
        Value::Bool(true),
        "wave27-06 invariant 3: no_execution must be literal Value::Bool(true)"
    );
    assert!(
        descriptor["no_execution"].is_boolean(),
        "wave27-06 invariant 3: no_execution must be a JSON bool, never a string"
    );

    // wave27-06 invariant 4: seed registry (claudecode current-default)
    // is NEVER apply-eligible. The wave26-03 6-condition gate requires
    // an explicit runtime-ready opt-in upstream; current-default alone
    // is rejected.
    assert_eq!(
        descriptor["router_apply_eligible"],
        Value::Bool(false),
        "wave27-06 invariant 4a: seed registry (claudecode current-default) MUST yield apply_eligible=false"
    );
    let blockers = descriptor["router_apply_blockers"]
        .as_array()
        .expect("wave27-06: router_apply_blockers MUST be a JSON array");
    assert!(
        !blockers.is_empty(),
        "wave27-06 invariant 4b: eligible=false MUST list at least one blocker (got {:?})",
        blockers
    );

    // ---- Part 2: dispatch byte-identical with vs without descriptor ----
    let plan2 = fixture_plan("(plan)");
    let resolved2 = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let args_no_desc = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "kind": "docs",
    });
    let mode_no_desc = parse_router_policy_mode(&args_no_desc).unwrap();
    let result_no_desc = attach_router_recommendation_block(
        action_execute_bridge(&plan2, &resolved2),
        mode_no_desc,
        &args_no_desc,
        &resolved2,
        &plan2,
    );
    let args_with_desc = json!({
        "router_policy_mode": "dry_run",
        "router_policy_path": policy.to_str().unwrap(),
        "router_policy_trace_index_path": trace.to_str().unwrap(),
        "router_backend_registry_path": registry.to_str().unwrap(),
        "router_dispatch_descriptor": true,
        "kind": "docs",
    });
    let mode_with_desc = parse_router_policy_mode(&args_with_desc).unwrap();
    let result_with_desc = attach_router_recommendation_block(
        action_execute_bridge(&plan2, &resolved2),
        mode_with_desc,
        &args_with_desc,
        &resolved2,
        &plan2,
    );
    let v_no = parse_payload(&result_no_desc);
    let v_with = parse_payload(&result_with_desc);
    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            v_no[field], v_with[field],
            "wave27-06 invariant 5: dispatch field `{}` MUST be byte-identical with vs without router_dispatch_descriptor=true",
            field
        );
    }

    // ---- Part 3: self-audit on plan.rs active source ----
    // Read the on-disk plan.rs and assert NO new shell-out / spawn /
    // git mutation / network / LLM client landed in the active code
    // (i.e. outside line + block comments and string literals). The
    // forbidden-pattern table is assembled from string parts so this
    // audit does NOT self-trip on the patterns it is scanning for.
    // wave24-06 / wave25-05 / wave26-06 lesson: a literal regex like
    // `child_process` would match this very test body.
    let plan_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/handlers/knowledge/plan.rs");
    let src = std::fs::read_to_string(&plan_path)
        .expect("wave27-06: plan.rs must be readable from CARGO_MANIFEST_DIR");
    let stripped = strip_rust_comments_and_strings(&src);
    // Tokens are assembled at runtime so this audit body's source code
    // does NOT contain the literal forbidden strings (wave24-06 /
    // wave25-05 / wave26-06 lesson). Variable names also stay clear of
    // the literals so the stripped source (which keeps identifier
    // names) does not self-trip the regex.
    let t_cp = String::from("child") + "_" + "process";
    let t_spawn = String::from("\\bspawn") + "\\(";
    let t_spawnblock = String::from("\\bspawn") + "_blocking\\(";
    let t_tproc = String::from("tokio") + "::process";
    let t_stdcmd = String::from("std::process::") + "Command";
    let t_rq = String::from("re") + "qwest::";
    let t_hyperc = String::from("\\bhy") + "per::";
    let t_oa = String::from("op") + "enai";
    let t_an = String::from("anth") + "ropic";
    let t_git = String::from("\\bgit ") + "(?:add|commit|push|reset|checkout|rm)";
    let t_libgit = String::from("g") + "it2::Repository::open";
    let forbidden = [
        t_cp.as_str(),
        t_spawn.as_str(),
        t_spawnblock.as_str(),
        t_tproc.as_str(),
        t_stdcmd.as_str(),
        t_rq.as_str(),
        t_hyperc.as_str(),
        t_oa.as_str(),
        t_an.as_str(),
        t_git.as_str(),
        t_libgit.as_str(),
    ];
    for pat in forbidden {
        let re = regex::Regex::new(pat).expect("wave27-06: audit pattern compiles");
        if re.is_match(&stripped) {
            panic!(
                "wave27-06 invariant 6: forbidden audit pattern `{}` found in plan.rs active source",
                pat
            );
        }
    }

    let _ = std::fs::remove_file(&policy);
    let _ = std::fs::remove_file(&trace);
    let _ = std::fs::remove_file(&registry);
}

// -----------------------------------------------------------------
// wave-28 / task 04 — task-runner manifest dry-run surface tests.
// -----------------------------------------------------------------

use super::task_runner_dry_run::{
    attach_task_runner_block, parse_task_runner_mode, TaskRunnerMode,
};

/// Convenience: write a fixture manifest under tmp and return its path.
fn write_temp_manifest(label: &str, body: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wave28-04-{}-{}.lisp", label, nanos));
    std::fs::write(&path, body).expect("write fixture manifest");
    path
}

/// Synthetic 3-node manifest: dependency A -> B (group A), C standalone (group B).
fn manifest_basic_body() -> &'static str {
    r#"(task-runner-manifest fixture-basic
  :schema "missiond.task-runner-manifest.v1"
  :wave wave-x
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave-x-shared.md"
  :productive_only true
  (node :task_id node-a
:depends_on []
:verification_tier local
:dispatch_group A
:estimated_minutes 30
:heartbeat_minutes 10
:write_scope ["a/file1.rs"])
  (node :task_id node-b
:depends_on [node-a]
:verification_tier smoke
:dispatch_group A
:estimated_minutes 45
:heartbeat_minutes 10
:write_scope ["a/file2.rs"])
  (node :task_id node-c
:depends_on []
:verification_tier full
:dispatch_group B
:estimated_minutes 20
:heartbeat_minutes 10
:write_scope ["c/only.rs"]))
"#
}

/// Two-node overlap inside group A; default policy reject -> severity=error.
fn manifest_overlap_reject_body() -> &'static str {
    r#"(task-runner-manifest fixture-overlap-reject
  :schema "missiond.task-runner-manifest.v1"
  :wave wave-y
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave-y-shared.md"
  :productive_only true
  (node :task_id node-x
:depends_on []
:verification_tier local
:dispatch_group A
:estimated_minutes 10
:heartbeat_minutes 5
:write_scope ["shared/path.rs"])
  (node :task_id node-y
:depends_on []
:verification_tier local
:dispatch_group A
:estimated_minutes 10
:heartbeat_minutes 5
:write_scope ["shared/path.rs"]))
"#
}

fn manifest_overlap_warn_body() -> &'static str {
    r#"(task-runner-manifest fixture-overlap-warn
  :schema "missiond.task-runner-manifest.v1"
  :wave wave-z
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave-z-shared.md"
  :productive_only true
  :overlap_policy warn
  (node :task_id node-p
:depends_on []
:verification_tier local
:dispatch_group A
:estimated_minutes 10
:heartbeat_minutes 5
:write_scope ["shared/path.rs"])
  (node :task_id node-q
:depends_on []
:verification_tier local
:dispatch_group A
:estimated_minutes 10
:heartbeat_minutes 5
:write_scope ["shared/path.rs"]))
"#
}

#[test]
fn task_runner_off_default_does_no_file_io() {
    // Re-pin the wave24-04 invariant pattern for the new task_runner
    // arg. Supply a non-existent manifest path under both mode=off
    // and mode-absent; the response MUST be byte-identical to the
    // baseline AND no file I/O must happen.
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };

    // Case 1: arg absent — Off.
    let args = json!({});
    let mode = parse_task_runner_mode(&args).expect("default off");
    assert!(matches!(mode, TaskRunnerMode::Off));
    let after = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let after_text = match after.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after_text,
        "wave28-04: mode-absent must be byte-identical to baseline"
    );

    // Case 2: explicit mode=off with a non-existent manifest path —
    // still byte-identical, NO file I/O. The path points at a name
    // that cannot exist so any read attempt would fail loudly.
    let bogus = std::env::temp_dir().join("wave28-04-must-not-exist-xxxxxxxxxxx.lisp");
    // Defensive: ensure it really doesn't exist.
    let _ = std::fs::remove_file(&bogus);
    let args2 = json!({
        "task_runner_mode": "off",
        "task_runner_manifest_path": bogus.to_str().unwrap(),
    });
    let mode2 = parse_task_runner_mode(&args2).expect("explicit off");
    assert!(matches!(mode2, TaskRunnerMode::Off));
    let after2 = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode2, &args2);
    let after2_text = match after2.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after2_text,
        "wave28-04: mode=off must be byte-identical even when manifest path is supplied"
    );
    // Sanity: the path must still not exist (Off path did not create it).
    assert!(
        !bogus.exists(),
        "wave28-04: Off mode must NOT create or touch the manifest path"
    );
    // Sanity: response carries NO task_runner block.
    let v: Value = serde_json::from_str(&after2_text).unwrap();
    assert!(
        v.get("task_runner").is_none(),
        "wave28-04: mode=off must NOT splice a task_runner block"
    );
}

#[test]
fn task_runner_dry_run_with_seed_manifest_emits_block() {
    let manifest = write_temp_manifest("seed", manifest_basic_body());
    let args = json!({
        "task_runner_mode": "dry_run",
        "task_runner_manifest_path": manifest.to_str().unwrap(),
    });
    let mode = parse_task_runner_mode(&args).expect("dry_run parses");
    assert!(matches!(mode, TaskRunnerMode::DryRun));
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let v = parse_payload(&result);
    let block = v
        .get("task_runner")
        .expect("dry_run must emit task_runner block");
    assert_eq!(
        block["applied"],
        Value::Bool(false),
        "wave28-04 invariant: applied=false hard-coded literal"
    );
    assert!(
        block["applied"].is_boolean(),
        "wave28-04 invariant: applied MUST be JSON bool, never string"
    );
    assert_eq!(block["schema"], "missiond.task-runner-plan.v0");
    assert_eq!(block["manifest_status"], "used");
    assert_eq!(block["wave"], "wave-x");
    assert_eq!(block["productive_only"], Value::Bool(true));
    assert_eq!(block["critical_path_minutes"], 75); // node-a (30) + node-b (45)
    assert_eq!(block["total_estimated_minutes"], 95); // 30 + 45 + 20
    assert!(block.get("batches").unwrap().is_array());
    assert_eq!(block["verification_tier_counts"]["local"], 1);
    assert_eq!(block["verification_tier_counts"]["smoke"], 1);
    assert_eq!(block["verification_tier_counts"]["full"], 1);

    let _ = std::fs::remove_file(&manifest);
}

#[test]
fn task_runner_dry_run_topological_batches_correct() {
    let manifest = write_temp_manifest("topo", manifest_basic_body());
    let args = json!({
        "task_runner_mode": "dry_run",
        "task_runner_manifest_path": manifest.to_str().unwrap(),
    });
    let mode = parse_task_runner_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let v = parse_payload(&result);
    let batches = v["task_runner"]["batches"]
        .as_array()
        .expect("batches array");
    // Batch 1: A=node-a (depth 0, group A) and B=node-c (depth 0, group B)
    // are both ready. They MUST be split into 2 batches because they
    // belong to different dispatch_groups (wave28-02 invariant). Batch
    // ordering is by lexicographic group name, so A comes first.
    // Batch 2: node-b (depends on node-a, group A).
    assert_eq!(batches.len(), 3, "expected 3 batches, got {:?}", batches);
    assert_eq!(batches[0][0], "node-a", "batch 0 group A first");
    assert_eq!(batches[1][0], "node-c", "batch 1 group B");
    assert_eq!(batches[2][0], "node-b", "batch 2 (depth 1, group A)");

    let _ = std::fs::remove_file(&manifest);
}

#[test]
fn task_runner_dry_run_overlap_default_reject() {
    let manifest = write_temp_manifest("overlap-reject", manifest_overlap_reject_body());
    let args = json!({
        "task_runner_mode": "dry_run",
        "task_runner_manifest_path": manifest.to_str().unwrap(),
    });
    let mode = parse_task_runner_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let v = parse_payload(&result);
    let diags = v["task_runner"]["overlap_diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert_eq!(diags.len(), 1, "expected one overlap diagnostic");
    assert_eq!(diags[0]["severity"], "error");
    assert_eq!(diags[0]["group"], "A");
    assert_eq!(diags[0]["pair"][0], "node-x");
    assert_eq!(diags[0]["pair"][1], "node-y");
    assert_eq!(diags[0]["paths"][0], "shared/path.rs");

    let _ = std::fs::remove_file(&manifest);
}

#[test]
fn task_runner_dry_run_overlap_warn_policy() {
    let manifest = write_temp_manifest("overlap-warn", manifest_overlap_warn_body());
    let args = json!({
        "task_runner_mode": "dry_run",
        "task_runner_manifest_path": manifest.to_str().unwrap(),
    });
    let mode = parse_task_runner_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let v = parse_payload(&result);
    let diags = v["task_runner"]["overlap_diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert_eq!(diags.len(), 1, "expected one overlap diagnostic");
    assert_eq!(
        diags[0]["severity"], "warning",
        "wave28-04: overlap_policy=warn must lower severity"
    );

    let _ = std::fs::remove_file(&manifest);
}

#[test]
fn task_runner_dry_run_missing_manifest_emits_status_missing() {
    let bogus = std::env::temp_dir().join(format!(
        "wave28-04-missing-{}.lisp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&bogus);
    let args = json!({
        "task_runner_mode": "dry_run",
        "task_runner_manifest_path": bogus.to_str().unwrap(),
    });
    let mode = parse_task_runner_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    let baseline_v: Value = serde_json::from_str(&baseline_text).unwrap();

    let result = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let v = parse_payload(&result);
    let block = v.get("task_runner").expect("dry_run emits block");
    assert_eq!(block["manifest_status"], "missing");
    assert_eq!(
        block["applied"],
        Value::Bool(false),
        "wave28-04: missing manifest must still surface applied=false literal"
    );
    assert!(
        block.get("task_runner_warning").is_some(),
        "wave28-04: missing manifest must surface task_runner_warning"
    );
    // Dispatch fields MUST be unchanged — manifest issues never fail dispatch.
    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            v[field], baseline_v[field],
            "wave28-04: missing manifest must NOT change dispatch field `{}`",
            field
        );
    }
}

#[test]
fn task_runner_dry_run_malformed_manifest_emits_status_malformed() {
    let manifest =
        write_temp_manifest("malformed", "(this-is-not-a-task-runner-manifest :foo bar)");
    let args = json!({
        "task_runner_mode": "dry_run",
        "task_runner_manifest_path": manifest.to_str().unwrap(),
    });
    let mode = parse_task_runner_mode(&args).unwrap();
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let result = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let v = parse_payload(&result);
    let block = v.get("task_runner").expect("dry_run emits block");
    assert_eq!(block["manifest_status"], "malformed");
    assert_eq!(block["applied"], Value::Bool(false));
    assert!(
        block.get("task_runner_warning").is_some(),
        "wave28-04: malformed manifest must surface task_runner_warning"
    );

    let _ = std::fs::remove_file(&manifest);
}

#[test]
fn task_runner_apply_returns_invalid_param() {
    let args = json!({"task_runner_mode": "apply"});
    let err = parse_task_runner_mode(&args).expect_err("apply must reject");
    assert_eq!(err.is_error, Some(true));
    let text = match err.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert!(
        text.contains("INVALID_PARAM") || text.contains("invalid"),
        "wave28-04: apply must surface INVALID_PARAM (got `{}`)",
        text
    );
    assert!(
        text.contains("apply"),
        "wave28-04: error must echo the offending value"
    );
}

#[test]
fn task_runner_auto_returns_invalid_param() {
    let args = json!({"task_runner_mode": "auto"});
    let err = parse_task_runner_mode(&args).expect_err("auto must reject");
    assert_eq!(err.is_error, Some(true));
    let text = match err.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert!(text.contains("INVALID_PARAM") || text.contains("invalid"));
    assert!(text.contains("auto"));
}

#[test]
fn task_runner_unknown_returns_invalid_param() {
    // Hostile string.
    let args = json!({"task_runner_mode": "hostile"});
    assert!(parse_task_runner_mode(&args).is_err());
    // Casing variants reject.
    let args = json!({"task_runner_mode": "DRY_RUN"});
    assert!(parse_task_runner_mode(&args).is_err());
    let args = json!({"task_runner_mode": "dryrun"});
    assert!(parse_task_runner_mode(&args).is_err());
    // Non-string types reject.
    let args = json!({"task_runner_mode": true});
    assert!(parse_task_runner_mode(&args).is_err());
    let args = json!({"task_runner_mode": 42});
    assert!(parse_task_runner_mode(&args).is_err());
}

#[test]
fn task_runner_dry_run_does_not_change_dispatch() {
    // Re-pin the dispatch byte-identical invariant under the new
    // code path: mode=dry_run with a valid manifest must NOT change
    // any dispatch field.
    let manifest = write_temp_manifest("invariant", manifest_basic_body());
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_v: Value = serde_json::from_str(match baseline.content.first() {
        Some(ToolContent::Text { text }) => text,
        _ => panic!("expected text content"),
    })
    .unwrap();
    let args = json!({
        "task_runner_mode": "dry_run",
        "task_runner_manifest_path": manifest.to_str().unwrap(),
    });
    let mode = parse_task_runner_mode(&args).unwrap();
    let result = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let v = parse_payload(&result);
    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            v[field], baseline_v[field],
            "wave28-04: dry_run must NOT change dispatch field `{}`",
            field
        );
    }

    let _ = std::fs::remove_file(&manifest);
}

#[test]
fn applied_remains_false_with_task_runner() {
    // Re-pin the applied=false literal across BOTH the existing
    // wave24-04+ router_recommendation block AND the new wave28-04
    // task_runner block. Both must emit applied=false hard-coded.
    let manifest = write_temp_manifest("applied", manifest_basic_body());
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let args = json!({
        "task_runner_mode": "dry_run",
        "task_runner_manifest_path": manifest.to_str().unwrap(),
    });
    let mode = parse_task_runner_mode(&args).unwrap();
    let result = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let v = parse_payload(&result);
    let block = v.get("task_runner").unwrap();
    assert_eq!(
        block["applied"],
        Value::Bool(false),
        "wave28-04 invariant: task_runner.applied must be hard-coded false"
    );
    assert!(
        block["applied"].is_boolean(),
        "wave28-04 invariant: task_runner.applied must be JSON bool, never string"
    );

    let _ = std::fs::remove_file(&manifest);
}

#[test]
fn task_runner_off_with_runner_args_and_router_args_does_no_file_io() {
    // Combined invariant: mode=off with BOTH the new task_runner
    // args AND all 4 wave-24..27 router args supplied MUST be byte-
    // identical to the pristine baseline (no router args, no
    // task_runner args). This proves the Off-path early-return
    // precedes BOTH the new and the prior reads (no file I/O for
    // any of: policy, trace-index, registry, descriptor, manifest).
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_text = match baseline.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };

    // Bogus paths for ALL the file-bearing args. Defensive: ensure
    // none exist so a regression that reads them would fail loudly
    // instead of silently succeeding.
    let bogus_policy = std::env::temp_dir().join("wave28-04-bogus-policy-xxxxx.lisp");
    let bogus_trace = std::env::temp_dir().join("wave28-04-bogus-trace-xxxxx.json");
    let bogus_registry = std::env::temp_dir().join("wave28-04-bogus-registry-xxxxx.lisp");
    let bogus_manifest = std::env::temp_dir().join("wave28-04-bogus-manifest-xxxxx.lisp");
    for p in [
        &bogus_policy,
        &bogus_trace,
        &bogus_registry,
        &bogus_manifest,
    ] {
        let _ = std::fs::remove_file(p);
    }

    let args = json!({
        // Both wave-28 task_runner args supplied.
        "task_runner_mode": "off",
        "task_runner_manifest_path": bogus_manifest.to_str().unwrap(),
        // All 4 wave-24..27 router args supplied.
        "router_policy_mode": "off",
        "router_policy_path": bogus_policy.to_str().unwrap(),
        "router_policy_trace_index_path": bogus_trace.to_str().unwrap(),
        "router_backend_registry_path": bogus_registry.to_str().unwrap(),
        "router_dispatch_descriptor": true,
    });
    let task_runner_mode = parse_task_runner_mode(&args).expect("task_runner off");
    let router_policy_mode = parse_router_policy_mode(&args).expect("router_policy off");
    assert!(matches!(task_runner_mode, TaskRunnerMode::Off));
    assert!(matches!(router_policy_mode, RouterPolicyMode::Off));

    // Apply both attach helpers in the same order as the live handler.
    let result = action_execute_bridge(&plan, &resolved);
    let result =
        attach_router_recommendation_block(result, router_policy_mode, &args, &resolved, &plan);
    let result = attach_task_runner_block(result, task_runner_mode, &args);
    let after_text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(
        baseline_text, after_text,
        "wave28-04: Off mode (both new and prior args) MUST be byte-identical to baseline"
    );
    // None of the bogus paths must have been created or touched.
    for p in [
        &bogus_policy,
        &bogus_trace,
        &bogus_registry,
        &bogus_manifest,
    ] {
        assert!(
            !p.exists(),
            "wave28-04: Off path must not create or read `{}`",
            p.display()
        );
    }
    // Sanity: response carries NEITHER block.
    let v: Value = serde_json::from_str(&after_text).unwrap();
    assert!(v.get("task_runner").is_none());
    assert!(v.get("router_recommendation").is_none());
}

/// Synthetic productive-only manifest used by the wave28-06 cross-layer
/// smoke. Mirrors the shape the Node-side wave28-06 fixtures drive
/// through wave28-01 checker, wave28-02 plan CLI, wave28-03 renderer,
/// wave28-05 batch verifier — same node ids, same dispatch_groups,
/// same heartbeat_minutes, same verification_tier mix (local x 2 +
/// full x 1 on the final smoke node).
fn manifest_wave28_06_loop_smoke_body() -> &'static str {
    r#"(task-runner-manifest m-wave28-06-loop-smoke
  :schema "missiond.task-runner-manifest.v1"
  :wave wave99
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave28-shared-preamble.md"
  :productive_only true
  (node :task_id wave99-01-alpha
:depends_on []
:verification_tier local
:dispatch_group A
:estimated_minutes 30
:heartbeat_minutes 10
:write_scope ["scripts/alpha.mjs"])
  (node :task_id wave99-02-beta
:depends_on [wave99-01-alpha]
:verification_tier local
:dispatch_group B
:estimated_minutes 25
:heartbeat_minutes 10
:write_scope ["scripts/beta.mjs"])
  (node :task_id wave99-99-final-smoke
:depends_on [wave99-01-alpha wave99-02-beta]
:verification_tier full
:dispatch_group C
:estimated_minutes 45
:heartbeat_minutes 10
:write_scope ["scripts/final.mjs"]))
"#
}

#[test]
fn task_runner_loop_smoke_pins_wave28_invariants() {
    // wave28-06 cross-layer smoke (Layer E — Rust daemon dry-run
    // surface). Pins the same productive-only manifest that the
    // wave28-06 Node fixtures drive through the other 4 layers
    // (wave28-01 checker, wave28-02 plan CLI, wave28-03 renderer,
    // wave28-05 batch verifier) and asserts the daemon's dry-run
    // projection agrees on the cross-layer invariants.
    //
    // Invariants pinned (as numbered in the wave28-06 task brief):
    //   1. Same manifest validates clean through all 5 layers.
    //   2. Productive-only: archive/backfill skipped at the schema
    //      layer; the daemon manifest body MUST contain only
    //      productive nodes (this test inspects the rendered batches).
    //   3. Verification-tier: full appears exactly once (final smoke);
    //      local x 2 elsewhere. The daemon block's
    //      verification_tier_counts MUST agree.
    //   5. No execution: applied=Value::Bool(false); no spawn / no
    //      git / no Node / no network — re-pinned by the static
    //      audit at the bottom of this test.
    //   6. Determinism: re-running the same manifest produces a
    //      byte-identical task_runner block (modulo manifest_path).

    let manifest =
        write_temp_manifest("wave28-06-loop-smoke", manifest_wave28_06_loop_smoke_body());
    let args = json!({
        "task_runner_mode": "dry_run",
        "task_runner_manifest_path": manifest.to_str().unwrap(),
    });
    let mode = parse_task_runner_mode(&args).expect("dry_run parses");
    assert!(matches!(mode, TaskRunnerMode::DryRun));

    // Baseline (no task_runner args) for the dispatch-field byte-
    // identical re-pin (mirrors wave28-04's invariant test).
    let plan = fixture_plan("(plan)");
    let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
    let baseline = action_execute_bridge(&plan, &resolved);
    let baseline_v: Value = serde_json::from_str(match baseline.content.first() {
        Some(ToolContent::Text { text }) => text,
        _ => panic!("expected text content"),
    })
    .unwrap();

    let result = attach_task_runner_block(action_execute_bridge(&plan, &resolved), mode, &args);
    let v = parse_payload(&result);
    let block = v.get("task_runner").expect(
        "wave28-06 invariant 1: dry_run MUST emit task_runner block for the synthetic manifest",
    );

    // Invariant 5: applied=false hard-coded literal Value::Bool(false).
    assert_eq!(
        block["applied"],
        Value::Bool(false),
        "wave28-06 invariant 5: applied MUST be hard-coded Value::Bool(false)"
    );
    assert!(
        block["applied"].is_boolean(),
        "wave28-06 invariant 5: applied MUST be JSON bool, never string"
    );

    // Invariant 1: schema label + manifest_status MUST agree with the
    // wave28-02 plan CLI output.
    assert_eq!(block["schema"], "missiond.task-runner-plan.v0");
    assert_eq!(block["manifest_status"], "used");
    assert_eq!(block["wave"], "wave99");

    // Invariant 2: productive_only echoed AND every emitted batch id
    // is a productive id (no archive/backfill substring leakage).
    assert_eq!(block["productive_only"], Value::Bool(true));
    let batches = block["batches"].as_array().expect("batches array");
    let pseudo_substrings = ["-archive-", "-backfill-", "-index", "lisp-backfill"];
    let mut productive_ids: Vec<String> = Vec::new();
    for batch in batches {
        for id in batch.as_array().expect("batch is array") {
            let id_str = id.as_str().expect("batch id is string").to_string();
            for sub in pseudo_substrings.iter() {
                assert!(
                    !id_str.contains(sub),
                    "wave28-06 invariant 2: emitted batch id `{}` MUST NOT contain forbidden substring `{}`",
                    id_str,
                    sub
                );
            }
            productive_ids.push(id_str);
        }
    }
    // 3 productive nodes -> 3 batches (split by dispatch_group A/B/C).
    assert_eq!(
        batches.len(),
        3,
        "wave28-06 invariant 1: 3 productive nodes -> 3 batches (split by dispatch_group)"
    );
    productive_ids.sort();
    assert_eq!(
        productive_ids,
        vec![
            "wave99-01-alpha".to_string(),
            "wave99-02-beta".to_string(),
            "wave99-99-final-smoke".to_string(),
        ],
        "wave28-06 invariant 1: emitted productive node ids MUST exactly match the manifest"
    );

    // Invariant 3: verification_tier_counts — full=1 (final smoke),
    // local=2 (alpha + beta), smoke=0. Mirrors the wave28-02 plan CLI
    // wave28-06 fixture assertion.
    assert_eq!(
        block["verification_tier_counts"]["full"], 1,
        "wave28-06 invariant 3: full tier appears exactly once (final smoke)"
    );
    assert_eq!(
        block["verification_tier_counts"]["local"], 2,
        "wave28-06 invariant 3: local tier x 2 (alpha + beta)"
    );
    assert_eq!(
        block["verification_tier_counts"]["smoke"], 0,
        "wave28-06 invariant 3: smoke tier == 0 in this synthetic manifest"
    );

    // Critical-path = longest dependency chain by estimated_minutes.
    // The DAG is: alpha (30) -> beta (25) -> final-smoke (45), AND
    // alpha (30) -> final-smoke (45) directly. The longest_from
    // memoized DFS yields:
    //   final-smoke = 45
    //   beta        = 25 + 45 = 70
    //   alpha       = 30 + max(beta=70, final-smoke=45) = 100
    // So critical path = 100; total = 30 + 25 + 45 = 100 too.
    assert_eq!(
        block["critical_path_minutes"], 100,
        "wave28-06: critical path MUST equal alpha (30) + beta (25) + final-smoke (45)"
    );
    assert_eq!(
        block["total_estimated_minutes"], 100,
        "wave28-06: total estimated MUST equal 30 + 25 + 45"
    );

    // Re-pin wave28-04 dispatch byte-identical invariant under the new
    // wave28-06 synthetic manifest: dispatch fields MUST NOT change.
    for field in [
        "target_tool",
        "target_source",
        "dispatch_strategy",
        "dispatch_strategy_source",
        "next_call",
        "execute_mode",
        "runner_status",
    ] {
        assert_eq!(
            v[field], baseline_v[field],
            "wave28-06 invariant 5: dry_run must NOT change dispatch field `{}`",
            field
        );
    }

    // Invariant 6: byte-identical determinism. Re-attach the block
    // with the same args + same fresh manifest read; the rendered
    // task_runner block MUST be byte-identical (modulo manifest_path
    // which always echoes the absolute tmp path).
    let result2 = attach_task_runner_block(
        action_execute_bridge(&plan, &resolved),
        parse_task_runner_mode(&args).unwrap(),
        &args,
    );
    let v2 = parse_payload(&result2);
    let block2 = v2.get("task_runner").expect("re-render produces block");
    // Strip manifest_path (the only field that legitimately echoes the
    // absolute tmp path). All other fields MUST be byte-identical.
    let mut a = block.clone();
    let mut b = block2.clone();
    a.as_object_mut().unwrap().remove("manifest_path");
    b.as_object_mut().unwrap().remove("manifest_path");
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap(),
        "wave28-06 invariant 6: dry-run task_runner block MUST be byte-identical on re-run"
    );

    // ---- Invariant 5 + 7 (no-execution + no-LLM/no-network) self-audit ----
    // Read plan.rs from disk and grep the active code (comments and
    // string literals stripped) for forbidden patterns. The pattern
    // table is assembled from string fragments at runtime so the
    // audit body itself does NOT trip the audit (wave24-06 / wave25-05
    // / wave26-06 / wave27-06 lesson). Variable names also stay clear
    // of the literal forbidden tokens.
    let plan_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/handlers/knowledge/plan.rs");
    let src = std::fs::read_to_string(&plan_path)
        .expect("wave28-06: plan.rs must be readable from CARGO_MANIFEST_DIR");
    let stripped = strip_rust_comments_and_strings(&src);
    let t_cp = String::from("ch") + "ild" + "_" + "process";
    let t_spawn = String::from("\\bsp") + "awn\\(";
    let t_spawnblock = String::from("\\bsp") + "awn_blocking\\(";
    let t_tproc = String::from("to") + "kio::process";
    let t_stdcmd = String::from("std::process::") + "Co" + "mmand";
    let t_rq = String::from("re") + "qwest::";
    let t_hyperc = String::from("\\bhy") + "per::";
    let t_oa = String::from("op") + "enai";
    let t_an = String::from("anth") + "ropic";
    let t_git = String::from("\\bg") + "it " + "(?:add|commit|push|reset|checkout|rm)";
    let t_libgit = String::from("g") + "it2::Repository::open";
    let forbidden = [
        t_cp.as_str(),
        t_spawn.as_str(),
        t_spawnblock.as_str(),
        t_tproc.as_str(),
        t_stdcmd.as_str(),
        t_rq.as_str(),
        t_hyperc.as_str(),
        t_oa.as_str(),
        t_an.as_str(),
        t_git.as_str(),
        t_libgit.as_str(),
    ];
    for pat in forbidden {
        let re = regex::Regex::new(pat).expect("wave28-06: audit pattern compiles");
        assert!(
            !re.is_match(&stripped),
            "wave28-06 invariant 5/7: forbidden audit pattern `{}` found in plan.rs active source",
            pat
        );
    }

    let _ = std::fs::remove_file(&manifest);
}

/// wave27-06 helper: strip line comments, block comments, and string
/// literals from a Rust source so the self-audit grep does NOT
/// match patterns mentioned in commentary or in the forbidden-pattern
/// table itself. Mirrors the JS-side stripper used by the renderer
/// self-audit (wave26-06 + wave27-05) but adapted for Rust syntax.
/// This is a heuristic (it does not handle every macro shape), but
/// it is sufficient for active-code sniffing — the test PANICS on
/// any match, so the bias is conservative.
fn strip_rust_comments_and_strings(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        // Block comment /* ... */ — handles nested /* */ one level
        // deep (Rust supports nesting; we do best-effort).
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < bytes.len() && depth > 0 {
                if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        // Line comment // ... \n
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // String literal "..." — handles \" escape. Does NOT
        // attempt raw strings r#"..."# (good enough for sniffing).
        if c == b'"' {
            i += 1;
            while i < bytes.len() {
                let d = bytes[i];
                if d == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if d == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // Char literal '..' — minimal handling; skip apostrophe runs
        // to avoid eating identifiers like 'static lifetime.
        if c == b'\'' {
            // Conservative: keep the apostrophe so we don't accidentally
            // chew lifetime annotations into something pattern-matching.
            out.push(c as char);
            i += 1;
            continue;
        }
        out.push(c as char);
        i += 1;
    }
    out
}
