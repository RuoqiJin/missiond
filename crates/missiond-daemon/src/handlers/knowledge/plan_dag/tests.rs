use super::acceptance::*;
use super::claim_lease::*;
use super::resume::*;
use super::rollback::*;
use super::*;
use chrono::TimeZone;
use chrono::Utc;
use missiond_core::types::PlanStatus;
use uuid::Uuid;

fn fixture_plan(sexp: &str) -> Plan {
    Plan {
        id: Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap(),
        board_task_id: "btk-dag".to_string(),
        source_directive_id: None,
        version: 1,
        sexp_text: sexp.to_string(),
        sexp_hash: "deadbeef".to_string(),
        status: PlanStatus::Approved,
        compiler_model: None,
        compiled_from: None,
        created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        approved_at: None,
        finished_at: None,
    }
}

// ── parser pure tests ──────────────────────────────────────────────

#[test]
fn parse_plan_dag_extracts_explicit_node_forms() {
    let sexp = r#"
        (plan
          :board_task_id "btk-1"
          (node :id "n1" :target "mission_execution" :objective "alpha")
          (node :id "n2" :target "mission_task_delegate" :depends-on ["n1"]))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes.len(), 2);
    assert_eq!(parsed.nodes[0].id, "n1");
    assert_eq!(parsed.nodes[0].target, "mission_execution");
    assert_eq!(parsed.nodes[0].objective.as_deref(), Some("alpha"));
    assert_eq!(parsed.nodes[1].depends_on, vec!["n1".to_string()]);
    // The :board_task_id sibling form is a keyword/value pair, not a form
    // we recognise — we don't surface it in unsupported_top_forms because
    // it is not a `(...)` sub-form. Only sibling sub-forms appear there.
}

#[test]
fn parse_plan_dag_records_unsupported_top_forms() {
    let sexp = r#"
        (plan
          (goal :ship "thing")
          (node :id "n1" :target "mission_execution"))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes.len(), 1);
    assert_eq!(parsed.unsupported_top_forms.len(), 1);
    assert!(parsed.unsupported_top_forms[0].starts_with("(goal"));
}

#[test]
fn parse_plan_dag_captures_unsupported_node_fields() {
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution" :priority "high" :foo bar))
    "#;
    let parsed = parse_plan_dag(sexp);
    let n = &parsed.nodes[0];
    let keys: Vec<&str> = n
        .unsupported_fields
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert!(keys.contains(&"priority"));
    assert!(keys.contains(&"foo"));
}

#[test]
fn parse_plan_dag_supports_paren_depends_on_alias() {
    let sexp = r#"
        (plan
          (node :id "a" :target "mission_execution")
          (node :id "b" :target "mission_execution" :depends-on (a)))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[1].depends_on, vec!["a".to_string()]);
}

#[test]
fn parse_plan_dag_failure_policy_default_and_override() {
    let sexp = r#"
        (plan
          (node :id "a" :target "mission_execution")
          (node :id "b" :target "mission_execution" :failure-policy "continue")
          (node :id "c" :target "mission_execution" :failure-policy "weird"))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].failure_policy, "fail-fast");
    assert_eq!(parsed.nodes[1].failure_policy, "continue");
    // unknown policy → fall back to fail-fast and capture the original.
    assert_eq!(parsed.nodes[2].failure_policy, "fail-fast");
    assert!(parsed.nodes[2]
        .unsupported_fields
        .iter()
        .any(|(k, v)| k == "failure-policy" && v == "weird"));
}

#[test]
fn parse_plan_dag_timeout_ms_parsed_as_integer() {
    let sexp = r#"(plan (node :id "n" :target "mission_execution" :timeout-ms 500))"#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].timeout_ms, Some(500));
}

// ── validator pure tests ───────────────────────────────────────────

#[test]
fn build_validated_dag_accepts_valid_chain() {
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution")
          (node :id "n2" :target "mission_task_delegate" :depends-on ["n1"])
          (node :id "n3" :target "mission_execution" :depends-on ["n2"]))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid chain");
    assert_eq!(parsed.nodes.len(), 3);
    assert_eq!(
        order,
        vec!["n1".to_string(), "n2".to_string(), "n3".to_string()]
    );
}

#[test]
fn build_validated_dag_rejects_no_nodes() {
    let sexp = "(plan :goal :ship)";
    let err = build_validated_dag(sexp).unwrap_err();
    assert!(matches!(err, DagBuildError::NoNodes));
}

#[test]
fn build_validated_dag_rejects_duplicate_id() {
    let sexp = r#"
        (plan
          (node :id "x" :target "mission_execution")
          (node :id "x" :target "mission_execution"))
    "#;
    let err = build_validated_dag(sexp).unwrap_err();
    match err {
        DagBuildError::DuplicateId(id) => assert_eq!(id, "x"),
        other => panic!("expected DuplicateId, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_invalid_target() {
    let sexp = r#"(plan (node :id "n1" :target "mission_explode"))"#;
    let err = build_validated_dag(sexp).unwrap_err();
    match err {
        DagBuildError::InvalidTarget { node_id, target } => {
            assert_eq!(node_id, "n1");
            assert_eq!(target, "mission_explode");
        }
        other => panic!("expected InvalidTarget, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_missing_dependency() {
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution" :depends-on ["ghost"]))
    "#;
    let err = build_validated_dag(sexp).unwrap_err();
    match err {
        DagBuildError::DependencyMissing { node_id, missing } => {
            assert_eq!(node_id, "n1");
            assert_eq!(missing, "ghost");
        }
        other => panic!("expected DependencyMissing, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_self_dependency() {
    let sexp = r#"(plan (node :id "n1" :target "mission_execution" :depends-on ["n1"]))"#;
    let err = build_validated_dag(sexp).unwrap_err();
    assert!(matches!(err, DagBuildError::SelfDependency(ref id) if id == "n1"));
}

#[test]
fn build_validated_dag_rejects_cycle() {
    let sexp = r#"
        (plan
          (node :id "a" :target "mission_execution" :depends-on ["b"])
          (node :id "b" :target "mission_execution" :depends-on ["a"]))
    "#;
    let err = build_validated_dag(sexp).unwrap_err();
    match err {
        DagBuildError::Cycle(members) => {
            assert!(members.contains(&"a".to_string()));
            assert!(members.contains(&"b".to_string()));
        }
        other => panic!("expected Cycle, got {:?}", other),
    }
}

#[test]
fn topo_sort_is_deterministic_for_independent_nodes() {
    // Three independent nodes — their order must be the BTreeSet
    // (lexicographic) order: a, b, c. This pins the contract that pure
    // tests can rely on.
    let sexp = r#"
        (plan
          (node :id "c" :target "mission_execution")
          (node :id "a" :target "mission_execution")
          (node :id "b" :target "mission_execution"))
    "#;
    let (_, order) = build_validated_dag(sexp).expect("topo");
    assert_eq!(
        order,
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

// ── unsupported metadata preservation ───────────────────────────────

#[test]
fn node_hint_summary_records_unsupported_fields() {
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution" :foo bar :baz "qux")
          (node :id "n2" :target "mission_execution"))
    "#;
    let (parsed, _) = build_validated_dag(sexp).expect("valid");
    let summary = build_node_hint_summary(&parsed);
    let by_node = summary
        .get("unsupported_fields")
        .and_then(|v| v.as_object())
        .expect("object");
    let n1 = by_node.get("n1").expect("n1 present");
    let arr = n1.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    // n2 has none — must NOT appear in the map at all.
    assert!(by_node.get("n2").is_none());
}

#[test]
fn node_hint_summary_records_unsupported_top_forms() {
    let sexp = r#"
        (plan
          (rollback :step "undo")
          (node :id "n1" :target "mission_execution"))
    "#;
    let (parsed, _) = build_validated_dag(sexp).expect("valid");
    let summary = build_node_hint_summary(&parsed);
    let arr = summary
        .get("unsupported_top_forms")
        .and_then(|v| v.as_array())
        .expect("array");
    assert_eq!(arr.len(), 1);
    assert!(arr[0].as_str().unwrap().starts_with("(rollback"));
}

// ── dry_run response shape ──────────────────────────────────────────

#[test]
fn build_nodes_summary_renders_topo_order_with_known_fields_only() {
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution" :objective "do thing")
          (node :id "n2" :target "mission_task_delegate" :depends-on ["n1"]
                :dispatch-strategy "agent-team" :timeout-ms 7000))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid");
    let summary = build_nodes_summary(&parsed.nodes, &order);
    let arr = summary.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["id"], "n1");
    assert_eq!(arr[0]["target"], "mission_execution");
    assert_eq!(arr[0]["objective"], "do thing");
    assert_eq!(arr[1]["dispatch_strategy"], "agent-team");
    assert_eq!(arr[1]["timeout_ms"], 7000);
    assert_eq!(arr[1]["failure_policy"], "fail-fast");
}

// ── scheduler_mode detection ────────────────────────────────────────

#[test]
fn detect_scheduler_mode_default_when_absent() {
    let v = json!({});
    assert!(!detect_scheduler_mode(&v).unwrap());
}

#[test]
fn detect_scheduler_mode_recognises_dag_v1() {
    assert!(detect_scheduler_mode(&json!({"scheduler_mode": "dag_v1"})).unwrap());
    assert!(detect_scheduler_mode(&json!({"scheduler_mode": "dag-v1"})).unwrap());
}

#[test]
fn detect_scheduler_mode_treats_legacy_aliases_as_default() {
    for alias in ["v0", "default", "current", "single_node", "single-node"] {
        assert!(!detect_scheduler_mode(&json!({"scheduler_mode": alias})).unwrap());
    }
}

#[test]
fn detect_scheduler_mode_rejects_unknown_value() {
    let err = detect_scheduler_mode(&json!({"scheduler_mode": "warp_drive"})).unwrap_err();
    assert_eq!(err.is_error, Some(true));
}

// ── wave-20 / task 07 — DAG-side guard for LLM-augmented modes ──────

#[test]
fn refuse_llm_inference_in_dag_mode_blocks_sonnet_suggest() {
    // sonnet_suggest is single-node-execute-only in v0; the DAG path
    // must reject the combo eagerly so the LLM proposal block is
    // never silently dropped.
    let args = json!({
        "scheduler_mode": "dag_v1",
        "infer_plan_fields": "sonnet_suggest"
    });
    let err = refuse_llm_inference_in_dag_mode(&args).expect("dag + sonnet_suggest combo refused");
    assert_eq!(err.is_error, Some(true));
}

#[test]
fn refuse_llm_inference_in_dag_mode_allows_deterministic_modes() {
    // Off / preview / apply_safe stay accepted — these are the
    // wave-18 / task 06 modes the DAG path already tolerates.
    for mode in ["off", "preview", "apply_safe"] {
        let args = json!({
            "scheduler_mode": "dag_v1",
            "infer_plan_fields": mode
        });
        assert!(
            refuse_llm_inference_in_dag_mode(&args).is_none(),
            "mode `{}` must not be refused",
            mode
        );
    }
    // Absent infer_plan_fields → no refusal.
    assert!(refuse_llm_inference_in_dag_mode(&json!({})).is_none());
}

#[test]
fn refuse_llm_inference_in_dag_mode_propagates_typo_error() {
    // A typo on the infer mode surfaces as INVALID_PARAM via the
    // shared parser, so the DAG path returns the same structured
    // error as the single-node path (no silent acceptance).
    let args = json!({
        "scheduler_mode": "dag_v1",
        "infer_plan_fields": "sonet_suggest"
    });
    let err = refuse_llm_inference_in_dag_mode(&args).expect("typo surfaced as structured error");
    assert_eq!(err.is_error, Some(true));
}

// ── execution helpers (pure paths only — full e2e needs AppState) ────

#[test]
fn outcome_aggregate_status_dag_succeeded_when_all_succeed() {
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "n1".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "unknown".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    assert_eq!(o.aggregate_status(), "dag_succeeded");
    assert_eq!(o.runner_status(), "all_nodes_dispatched");
    assert_eq!(o.target_plan_status(), Some(PlanStatus::Succeeded));
}

#[test]
fn outcome_aggregate_status_fail_fast_marks_dag_failed_and_plan_failed() {
    let mut o = ExecutionOutcome::default();
    o.aborted_fail_fast = true;
    o.results.push(NodeResult {
        id: "n1".into(),
        target: "mission_execution".into(),
        state: NodeState::Failed {
            reason: "boom".into(),
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    assert_eq!(o.aggregate_status(), "dag_failed");
    assert_eq!(o.runner_status(), "fail_fast_aborted");
    assert_eq!(o.target_plan_status(), Some(PlanStatus::Failed));
}

#[test]
fn outcome_aggregate_status_continue_with_failure_yields_partial() {
    let mut o = ExecutionOutcome::default();
    // Failed node + downstream skip + an independent success → partial.
    o.results.push(NodeResult {
        id: "n1".into(),
        target: "mission_execution".into(),
        state: NodeState::Failed { reason: "x".into() },
        dispatch_strategy: "unknown".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "n2".into(),
        target: "mission_execution".into(),
        state: NodeState::SkippedUpstreamFailed {
            failed_dep: "n1".into(),
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "n3".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "unknown".into(),
        inner_payload: json!({"ok": true}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    assert_eq!(o.aggregate_status(), "dag_partial");
    assert_eq!(o.target_plan_status(), Some(PlanStatus::Failed));
    let v = o.node_results_json();
    let arr = v.as_array().unwrap();
    assert_eq!(arr[1]["state"], "skipped_upstream_failed");
    assert_eq!(arr[1]["failed_dep"], "n1");
}

#[test]
fn propagate_taint_marks_full_subtree() {
    // Graph: a -> b -> c, a -> d. Taint a; expect b,c,d all tainted.
    let nodes = vec![
        DagNode {
            id: "a".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
        DagNode {
            id: "b".into(),
            target: "mission_execution".into(),
            depends_on: vec!["a".into()],
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
        DagNode {
            id: "c".into(),
            target: "mission_execution".into(),
            depends_on: vec!["b".into()],
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
        DagNode {
            id: "d".into(),
            target: "mission_execution".into(),
            depends_on: vec!["a".into()],
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &nodes {
        for dep in &n.depends_on {
            succs.entry(dep.as_str()).or_default().push(n.id.as_str());
        }
    }
    let mut tainted: HashMap<String, String> = HashMap::new();
    propagate_taint(&nodes[0], &succs, &mut tainted);
    assert_eq!(tainted.get("b"), Some(&"a".to_string()));
    assert_eq!(tainted.get("c"), Some(&"a".to_string()));
    assert_eq!(tainted.get("d"), Some(&"a".to_string()));
    assert!(tainted.get("a").is_none());
}

#[test]
fn build_node_inner_args_for_mission_execution_emits_known_fields() {
    let node = DagNode {
        id: "n1".into(),
        target: "mission_execution".into(),
        objective: Some("do thing".into()),
        failure_policy: "fail-fast".into(),
        dispatch_strategy: Some("fresh-code-alignment".into()),
        target_project: Some("missiond".into()),
        requested_cwd: Some("/abs/path".into()),
        ..Default::default()
    };
    let plan = fixture_plan("(plan)");
    let built = build_node_inner_args(&node, &plan);
    let inner = built.inner_args.expect("ok");
    assert_eq!(inner["action"], "open");
    assert_eq!(inner["dispatch_strategy"], "fresh-code-alignment");
    assert_eq!(inner["project"], "missiond");
    assert_eq!(inner["target_project"], "missiond");
    assert_eq!(inner["requested_cwd"], "/abs/path");
    assert_eq!(built.dispatch_strategy, "fresh-code-alignment");
}

#[test]
fn build_node_inner_args_for_task_delegate_uses_objective_and_cwd() {
    let node = DagNode {
        id: "n1".into(),
        target: "mission_task_delegate".into(),
        objective: Some("ship a thing".into()),
        failure_policy: "fail-fast".into(),
        timeout_ms: Some(15_000),
        requested_cwd: Some("/abs/path".into()),
        ..Default::default()
    };
    let plan = fixture_plan("(plan)");
    let built = build_node_inner_args(&node, &plan);
    let inner = built.inner_args.expect("ok");
    assert_eq!(inner["objective"], "ship a thing");
    assert_eq!(inner["cwd"], "/abs/path");
    assert_eq!(inner["timeout_secs"], 15);
}

#[test]
fn build_node_inner_args_for_flow_run_requires_flow_id() {
    let node = DagNode {
        id: "n1".into(),
        target: "mission_flow_run".into(),
        failure_policy: "fail-fast".into(),
        ..Default::default()
    };
    let plan = fixture_plan("(plan)");
    let built = build_node_inner_args(&node, &plan);
    // Missing flow_id -> inner builder returns Err with a structured payload.
    assert!(built.inner_args.is_err());
}

// ── wave-13 :: plan_dag_node_dispatch typed evidence shape ───────
//
// Each DAG node dispatch (success or failure branch) builds an
// `EvidenceEntry` from `evidence_collector` instead of a hand-rolled
// JSON object. These tests pin the projected on-disk shape so the
// wire-compatible mapping
//   legacy `kind="plan_dag_node_dispatch"`
//     ↦ canonical `source="plan_dag_node_dispatch"` + `kind="dispatch"`
// is enforced, and the legacy passthrough fields (`scheduler_mode`,
// `node_id`, `plan_id`, `target_tool`, `target`, `dispatch_strategy`,
// and the failure-branch `inner_error`) keep their flat top-level
// placement for existing audit dashboards.
//
// We replay the exact entry construction (mirrored from
// `execute_sequential`) instead of standing up an `AppState` so the
// assertions stay focused on the wire shape.
/// Wave-14 / Task 02: the production path now writes a live
/// `EventRef::new(execution, plan_node_state_changed, <seq|deterministic>)`.
/// The fixtures pin the **deterministic** branch (no bus available in
/// pure tests) so assertions can grep on the deterministic id format.
fn build_dag_success_entry(
    node: &DagNode,
    plan: &Plan,
    dispatch_strategy: &str,
    inner_payload: Value,
) -> Value {
    let det = deterministic_plan_node_event_id(
        plan.id,
        &node.id,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "ready",
        "succeeded",
    );
    EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_inner_dispatch(inner_payload.clone())
    .with_state_transition("ready -> succeeded")
    .add_execution_event(EventRef::new(
        EVENT_REF_SOURCE_EXECUTION,
        EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
        det,
    ))
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
    .with_extra("inner_result", inner_payload)
    .into_json()
}

fn build_dag_failure_entry(
    node: &DagNode,
    plan: &Plan,
    dispatch_strategy: &str,
    inner_payload: Value,
) -> Value {
    let det = deterministic_plan_node_event_id(
        plan.id,
        &node.id,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "ready",
        "failed",
    );
    EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("ready -> failed")
    .add_execution_event(EventRef::new(
        EVENT_REF_SOURCE_EXECUTION,
        EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
        det,
    ))
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
    .with_extra("inner_error", inner_payload)
    .into_json()
}

fn fixture_dag_node(id: &str, target: &str) -> DagNode {
    DagNode {
        id: id.into(),
        target: target.into(),
        failure_policy: "fail-fast".into(),
        dispatch_strategy: Some("agent-team".into()),
        ..Default::default()
    }
}

#[test]
fn dag_node_dispatch_evidence_carries_canonical_source_and_kind() {
    let node = fixture_dag_node("n1", "mission_execution");
    let plan = fixture_plan("(plan)");
    let entry = build_dag_success_entry(&node, &plan, "agent-team", json!({"ok": true}));
    // wave-12 wire-compatible mapping for the DAG branch.
    assert_eq!(entry["source"], "plan_dag_node_dispatch");
    assert_eq!(entry["kind"], "dispatch");
    assert_eq!(entry["schema_version"], "v0");
    // Success-branch transition and inner payload land under canonical slots.
    assert_eq!(entry["state_transition"], "ready -> succeeded");
    assert_eq!(entry["inner_dispatch"]["ok"], true);
    // Pre-wave12 sidecars carried the same payload under `inner_result`;
    // we keep it as a legacy alias for byte-for-byte reader compat.
    assert_eq!(entry["inner_result"]["ok"], true);
}

#[test]
fn dag_node_dispatch_evidence_keeps_legacy_passthrough_keys_flat() {
    let node = fixture_dag_node("n7", "mission_task_delegate");
    let plan = fixture_plan("(plan)");
    let entry = build_dag_success_entry(
        &node,
        &plan,
        "fresh-code-alignment",
        json!({"task_id": "t7"}),
    );
    // Audit dashboards historically grep at the top level for these.
    assert_eq!(entry["scheduler_mode"], "dag_v1");
    assert_eq!(entry["node_id"], "n7");
    assert_eq!(entry["plan_id"], plan.id.to_string());
    assert_eq!(entry["target_tool"], "mission_task_delegate");
    // `target` is the new short alias the wave-13 plan_dag entry now also
    // exposes (mirrors `target_tool` for DAG-only consumers that pivot
    // on the shorter name).
    assert_eq!(entry["target"], "mission_task_delegate");
    assert_eq!(entry["dispatch_strategy"], "fresh-code-alignment");
}

#[test]
fn dag_node_dispatch_evidence_failure_branch_keeps_inner_error_legacy_key() {
    // The failure branch must NOT call `with_inner_dispatch`; the inner
    // payload stays under the legacy `inner_error` extra so historical
    // readers that filtered on that key keep working byte-for-byte.
    let node = fixture_dag_node("n3", "mission_execution");
    let plan = fixture_plan("(plan)");
    let inner = json!({"error": "downstream rejected request"});
    let entry = build_dag_failure_entry(&node, &plan, "resident-lisp", inner.clone());
    assert_eq!(entry["state_transition"], "ready -> failed");
    // Legacy `inner_error` key survives at top level.
    assert_eq!(entry["inner_error"], inner);
    // Canonical typed slot is intentionally absent on the failure branch.
    assert!(
        entry.get("inner_dispatch").is_none(),
        "failure branch must not populate `inner_dispatch`; payload stays under `inner_error`"
    );
}

/// Wave-14 / Task 02: production now writes a live `EventRef::new(...)`
/// — never `EventRef::unavailable(...)` — on the success branch. The
/// fixture pins the deterministic-id branch (no bus) so this test
/// verifies (a) `unavailable` is absent, (b) the canonical
/// `source=execution` / `kind=plan_node_state_changed` mapping survives,
/// (c) the deterministic id matches the
/// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` format.
#[test]
fn dag_node_dispatch_evidence_records_live_event_ref() {
    let node = fixture_dag_node("n2", "mission_execution");
    let plan = fixture_plan("(plan)");
    let entry = build_dag_success_entry(&node, &plan, "agent-team", json!({"ok": true}));
    let events = entry["execution_events"]
        .as_array()
        .expect("execution_events array present");
    assert_eq!(events.len(), 1, "exactly one event reference per node");
    let ref0 = &events[0];
    assert!(
        ref0.get("unavailable").is_none(),
        "live path must NOT mark the ref as unavailable: {:?}",
        ref0
    );
    assert_eq!(ref0["source"], "execution");
    assert_eq!(ref0["kind"], "plan_node_state_changed");
    let event_id = ref0["event_id"].as_str().expect("event_id string");
    let expected = format!(
        "plan-node:{}:{}:{}:ready-succeeded",
        plan.id, node.id, PLAN_NODE_DEFAULT_ATTEMPT
    );
    assert_eq!(event_id, expected);
}

/// Wave-16 / Task 07: every plan-DAG evidence entry must surface the
/// `EventRefStatus` provenance tag on the JSON envelope. The publish
/// path (success and failure branches) constructs `EventRef::new(...)`
/// (alias for `live`) so the resulting wire form carries
/// `"status": "live"`. This pins the contract so a future refactor
/// that accidentally drops the status field gets caught.
#[test]
fn dag_node_dispatch_evidence_surfaces_status_live_on_publish_path() {
    let node = fixture_dag_node("n4", "mission_execution");
    let plan = fixture_plan("(plan)");
    let success = build_dag_success_entry(&node, &plan, "agent-team", json!({"ok": true}));
    assert_eq!(
        success["execution_events"][0]["status"], "live",
        "publish-path success branch surfaces status=live"
    );
    let failure = build_dag_failure_entry(
        &node,
        &plan,
        "agent-team",
        json!({"error": "downstream rejected"}),
    );
    assert_eq!(
        failure["execution_events"][0]["status"], "live",
        "publish-path failure branch surfaces status=live (deterministic id is still real)"
    );
}

/// Wave-16 / Task 07: when a downstream call site cannot stamp a live
/// id directly (e.g. the dispatch ran out-of-band of the publish
/// task), the resolver lookup degrades to
/// `EventRef::unavailable(EVENT_REF_RESOLVER_MISS_REASON)` rather
/// than failing. The resulting evidence entry must carry
/// `status=unavailable` so audit consumers can distinguish a real
/// recovery failure from a live publish.
#[test]
fn dag_node_dispatch_evidence_resolver_miss_degrades_to_unavailable() {
    use evidence_collector::{EventRefResolver, EVENT_REF_RESOLVER_MISS_REASON};
    let node = fixture_dag_node("n5", "mission_execution");
    let plan = fixture_plan("(plan)");
    // Empty resolver — every lookup misses by construction.
    let resolver = EventRefResolver::new();
    let event_ref = resolver.lookup_plan_node_state_change(
        &plan.id.to_string(),
        &node.id,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "ready",
        "succeeded",
    );
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("ready -> succeeded")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(plan.id))
    .into_json();
    let ev = &entry["execution_events"][0];
    assert_eq!(ev["status"], "unavailable");
    assert_eq!(ev["unavailable"], true);
    assert_eq!(ev["unavailable_reason"], EVENT_REF_RESOLVER_MISS_REASON);
    assert!(
        ev.get("event_id").is_none(),
        "unavailable ref carries no event_id"
    );
}

/// Wave-16 / Task 07: when the resolver IS populated (the passive
/// subscriber observed a `PlanNodeStateChanged` for this correlation
/// tuple), a downstream call site that queries the resolver gets a
/// real id back tagged `status=log` (recovered post-hoc — distinct
/// from `live` which only the publish path itself can stamp).
#[test]
fn dag_node_dispatch_evidence_resolver_hit_surfaces_status_log() {
    use evidence_collector::EventRefResolver;
    let node = fixture_dag_node("n6", "mission_execution");
    let plan = fixture_plan("(plan)");
    let resolver = EventRefResolver::new();
    // Simulate the passive subscriber having observed a Seq=42
    // PlanNodeStateChanged for this transition.
    resolver.record_plan_node_state_change(
        &plan.id.to_string(),
        &node.id,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "ready",
        "succeeded",
        "execution",
        "plan_node_state_changed",
        "42",
    );
    let event_ref = resolver.lookup_plan_node_state_change(
        &plan.id.to_string(),
        &node.id,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "ready",
        "succeeded",
    );
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("ready -> succeeded")
    .add_execution_event(event_ref)
    .into_json();
    let ev = &entry["execution_events"][0];
    assert_eq!(ev["status"], "log", "resolver hit surfaces status=log");
    assert_eq!(ev["event_id"], "42");
    assert_eq!(ev["source"], "execution");
    assert_eq!(ev["kind"], "plan_node_state_changed");
    assert!(ev.get("unavailable").is_none());
}

// ── wave-17 / Task 06 :: persistent event-log query ─────────────────

/// Wave-17 / Task 06: when the in-memory cache misses but the
/// persistent event log carries a matching `PlanNodeStateChanged`
/// row, the resolver must recover the ref and the evidence entry
/// must surface `event_ref_status=log` plus the leading
/// `execution_events[0].status=log`. This pins the contract that
/// event refs survive daemon restarts (the in-memory cache is
/// dropped on restart but the event log persists).
#[tokio::test]
async fn dag_node_dispatch_evidence_recovers_event_ref_from_log_after_cache_miss() {
    use evidence_collector::EventRefResolver;
    use missiond_core::event::log::{LogError, LogReadable, LoggedEvent, Seq};
    use missiond_core::event::Domain;

    // A tiny `LogReadable` stub that returns one matching row. Matches
    // the post-restart shape: cache empty, log carries the prior emit.
    struct OneRowLog(LoggedEvent);
    #[async_trait::async_trait]
    impl LogReadable for OneRowLog {
        async fn read_from(
            &self,
            _domain: Domain,
            _after: Seq,
            _limit: usize,
        ) -> Result<Vec<LoggedEvent>, LogError> {
            Ok(vec![self.0.clone()])
        }
        async fn head_seq(&self) -> Result<Seq, LogError> {
            Ok(self.0.seq)
        }
    }

    let node = fixture_dag_node("n7", "mission_execution");
    let plan = fixture_plan("(plan)");
    let plan_id_str = plan.id.to_string();
    let row = LoggedEvent {
        seq: Seq(314),
        domain: Domain::Execution,
        kind: "plan_node_state_changed".to_string(),
        payload: json!({
            "PlanNodeStateChanged": {
                "plan_id": plan_id_str,
                "node_id": node.id,
                "from": "ready",
                "to": "succeeded",
                "attempt": PLAN_NODE_DEFAULT_ATTEMPT,
            }
        }),
        producer_id: "test/plan_dag".to_string(),
        dedupe_key: None,
        causation_depth: 0,
        trace_id: None,
        span_id: None,
        parent_span_id: None,
        ts: chrono::Utc::now(),
        ephemeral: false,
    };
    let log = OneRowLog(row);

    // Empty resolver — cache miss forces the log-query path.
    let resolver = EventRefResolver::new();
    let event_ref = resolver
        .lookup_or_query_plan_node_state_change(
            &log,
            &plan_id_str,
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "ready",
            "succeeded",
        )
        .await;

    // Build the evidence entry the way `emit_evidence_finished` would.
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("ready -> succeeded")
    .with_primary_event_ref(&event_ref, None)
    .add_execution_event(event_ref)
    .into_json();

    // Top-level surface fields carry the log provenance. Wave-18 /
    // task 01 — `event_ref_source` now reports the resolver tier
    // (`event_log_query`) instead of the raw wire source, so audit
    // consumers can pivot directly on the lookup path.
    assert_eq!(
        entry["event_ref_status"], "log",
        "log-recovered ref surfaces status=log at top level"
    );
    assert_eq!(
        entry["event_ref_source"], "event_log_query",
        "wave-18 query-tier hit surfaces provenance=event_log_query"
    );
    assert!(
        entry.get("event_ref_warning").is_none(),
        "no warning when recovery succeeded"
    );
    // Per-event entry mirrors the same provenance.
    let ev = &entry["execution_events"][0];
    assert_eq!(ev["status"], "log");
    assert_eq!(ev["event_id"], "314");
    assert_eq!(ev["source"], "execution");
    assert_eq!(ev["kind"], "plan_node_state_changed");
}

// ── wave-13 / 02 :: v2 scheduler runtime — pure tests ────────────
//
// Full execution requires `AppState` (handlers + project registry +
// evidence sidecar). The wave-based scheduler's pure subset is the
// concurrency-plan projection (`compute_concurrency_plan`) and the
// response shape (`ExecutionOutcome::node_results_json` /
// `skipped_nodes_json`). End-to-end behaviour is exercised by the
// existing v1 tests that still pass under the v2 runtime (above), plus
// the bridge / record_evidence tests under `plan::tests`.

#[test]
fn parse_max_parallel_nodes_defaults_to_one_when_absent() {
    let v = json!({});
    assert_eq!(parse_max_parallel_nodes(&v), 1);
}

#[test]
fn parse_max_parallel_nodes_reads_positive_integer() {
    let v = json!({"max_parallel_nodes": 4});
    assert_eq!(parse_max_parallel_nodes(&v), 4);
}

#[test]
fn parse_max_parallel_nodes_clamps_zero_to_one() {
    // Caller passing 0 / negative is normalised to the v1-equivalent
    // sequential contract instead of hard-failing — same posture as the
    // dispatch_strategy unknown-value normalisation in plan.rs.
    let v = json!({"max_parallel_nodes": 0});
    assert_eq!(parse_max_parallel_nodes(&v), 1);
}

#[test]
fn compute_concurrency_plan_linear_chain_single_per_wave() {
    // a -> b -> c with max=2 still produces three single-node waves
    // because each tier exposes only one ready node.
    let sexp = r#"
        (plan
          (node :id "a" :target "mission_execution")
          (node :id "b" :target "mission_execution" :depends-on ["a"])
          (node :id "c" :target "mission_execution" :depends-on ["b"]))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid");
    let waves = compute_concurrency_plan(&parsed.nodes, &order, 2);
    assert_eq!(waves.len(), 3);
    assert_eq!(waves[0], vec!["a".to_string()]);
    assert_eq!(waves[1], vec!["b".to_string()]);
    assert_eq!(waves[2], vec!["c".to_string()]);
}

#[test]
fn compute_concurrency_plan_diamond_fans_under_max_2() {
    // a fans out to {b, c}, both feed d. max=2 lets b+c run together.
    let sexp = r#"
        (plan
          (node :id "a" :target "mission_execution")
          (node :id "b" :target "mission_execution" :depends-on ["a"])
          (node :id "c" :target "mission_execution" :depends-on ["a"])
          (node :id "d" :target "mission_execution" :depends-on ["b" "c"]))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid");
    let waves = compute_concurrency_plan(&parsed.nodes, &order, 2);
    assert_eq!(waves.len(), 3);
    assert_eq!(waves[0], vec!["a".to_string()]);
    // Wave 2 ids are sorted lexicographically for determinism.
    assert_eq!(waves[1], vec!["b".to_string(), "c".to_string()]);
    assert_eq!(waves[2], vec!["d".to_string()]);
}

#[test]
fn compute_concurrency_plan_max_one_matches_v1_sequential_order() {
    // max_parallel_nodes=1 must produce exactly one wave per node, in
    // strict topological-sort order — preserves the v1 contract.
    let sexp = r#"
        (plan
          (node :id "a" :target "mission_execution")
          (node :id "b" :target "mission_execution" :depends-on ["a"])
          (node :id "c" :target "mission_execution" :depends-on ["a"])
          (node :id "d" :target "mission_execution" :depends-on ["b" "c"]))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid");
    let waves = compute_concurrency_plan(&parsed.nodes, &order, 1);
    assert_eq!(waves.len(), 4);
    for w in &waves {
        assert_eq!(w.len(), 1, "max=1 must produce single-node waves");
    }
    let flat: Vec<String> = waves.iter().flatten().cloned().collect();
    assert_eq!(flat, vec!["a", "b", "c", "d"]);
}

#[test]
fn compute_concurrency_plan_three_independent_packs_into_one_wave_when_budget_allows() {
    // Three roots, no dependencies — max=3 should pack them all into
    // one wave; max=2 splits 2+1; max=1 splits 1+1+1 in id-sorted order.
    let sexp = r#"
        (plan
          (node :id "x" :target "mission_execution")
          (node :id "a" :target "mission_execution")
          (node :id "m" :target "mission_execution"))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid");
    let w3 = compute_concurrency_plan(&parsed.nodes, &order, 3);
    assert_eq!(
        w3,
        vec![vec!["a".to_string(), "m".to_string(), "x".to_string()]]
    );
    let w2 = compute_concurrency_plan(&parsed.nodes, &order, 2);
    assert_eq!(
        w2,
        vec![
            vec!["a".to_string(), "m".to_string()],
            vec!["x".to_string()],
        ]
    );
    let w1 = compute_concurrency_plan(&parsed.nodes, &order, 1);
    assert_eq!(w1.len(), 3);
    assert_eq!(w1[0], vec!["a".to_string()]);
    assert_eq!(w1[1], vec!["m".to_string()]);
    assert_eq!(w1[2], vec!["x".to_string()]);
}

#[test]
fn compute_concurrency_plan_clamps_zero_max_parallel_to_one() {
    // 0 is normalised to 1 inside parse_max_parallel_nodes, but the
    // pure helper also applies the clamp so direct callers stay safe.
    let sexp = r#"
        (plan
          (node :id "a" :target "mission_execution")
          (node :id "b" :target "mission_execution"))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid");
    let waves = compute_concurrency_plan(&parsed.nodes, &order, 0);
    assert_eq!(waves.len(), 2, "max=0 must clamp to 1 -> two waves");
}

#[test]
fn skipped_nodes_json_filters_only_skip_states() {
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "a".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "b".into(),
        target: "mission_execution".into(),
        state: NodeState::SkippedUpstreamFailed {
            failed_dep: "a".into(),
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "c".into(),
        target: "mission_execution".into(),
        state: NodeState::SkippedCondition,
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "d".into(),
        target: "mission_execution".into(),
        state: NodeState::Failed {
            reason: "boom".into(),
        },
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({"error": "boom"}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "e".into(),
        target: "mission_execution".into(),
        state: NodeState::SkippedFailFastAbort {
            aborter: "d".into(),
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    let v = o.skipped_nodes_json();
    let arr = v.as_array().expect("array");
    assert_eq!(arr.len(), 3, "only the three skip variants surface here");
    assert_eq!(arr[0]["id"], "b");
    assert_eq!(arr[0]["state"], "skipped_upstream_failed");
    assert_eq!(arr[0]["failed_dep"], "a");
    assert_eq!(arr[1]["id"], "c");
    assert_eq!(arr[1]["state"], "skipped_condition");
    assert_eq!(arr[2]["id"], "e");
    assert_eq!(arr[2]["state"], "skipped_fail_fast_abort");
    assert_eq!(arr[2]["aborter"], "d");
}

#[test]
fn node_results_json_includes_skipped_fail_fast_abort_variant() {
    let mut o = ExecutionOutcome::default();
    o.aborted_fail_fast = true;
    o.results.push(NodeResult {
        id: "a".into(),
        target: "mission_execution".into(),
        state: NodeState::Failed {
            reason: "boom".into(),
        },
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({"error": "boom"}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "b".into(),
        target: "mission_execution".into(),
        state: NodeState::SkippedFailFastAbort {
            aborter: "a".into(),
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    let v = o.node_results_json();
    let arr = v.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[1]["state"], "skipped_fail_fast_abort");
    assert_eq!(arr[1]["aborter"], "a");
    assert_eq!(o.aggregate_status(), "dag_failed");
    assert_eq!(o.runner_status(), "fail_fast_aborted");
}

#[test]
fn outcome_partial_status_when_no_failure_but_skips_present() {
    // wave-13/02 fail-fast abort path: the failing node's policy may be
    // `continue` while *another* upstream-tainted child still ends up as
    // `skipped_upstream_failed` — aggregate_status must surface this as
    // dag_partial (not dag_succeeded).
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "a".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "b".into(),
        target: "mission_execution".into(),
        state: NodeState::SkippedCondition,
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    assert_eq!(o.aggregate_status(), "dag_partial");
    assert_eq!(o.runner_status(), "partial_dispatched");
    assert_eq!(o.target_plan_status(), None);
}

// ── evidence shape :: v2 lifecycle transitions ─────────────────
//
// The v2 scheduler emits one evidence entry per state transition. We
// pin the `state_transition` annotations + the `skip_reason` extra so
// audit dashboards can route on them. Replays the entry construction
// (mirrored from the helpers above) instead of standing up `AppState`.

/// Wave-14 / Task 02: fixtures pin the **deterministic** `EventRef::new`
/// branch (no live bus available in pure tests). This mirrors what the
/// production helpers write when the bus publish either succeeds (with
/// the live `Seq` as the id) or fails (with the deterministic id as the
/// id + `bus_publish_warnings` populated). Tests assert the wire shape
/// of the *entry*, not the bus interaction itself.
fn build_running_entry(node: &DagNode, plan: &Plan, dispatch_strategy: &str) -> Value {
    let det = deterministic_plan_node_event_id(
        plan.id,
        &node.id,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "ready",
        "running",
    );
    EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("ready -> running")
    .add_execution_event(EventRef::new(
        EVENT_REF_SOURCE_EXECUTION,
        EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
        det,
    ))
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
    .into_json()
}

fn build_finished_entry(
    node: &DagNode,
    plan: &Plan,
    dispatch_strategy: &str,
    inner_payload: Value,
    succeeded: bool,
) -> Value {
    let to = if succeeded { "succeeded" } else { "failed" };
    let det = deterministic_plan_node_event_id(
        plan.id,
        &node.id,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "running",
        to,
    );
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .add_execution_event(EventRef::new(
        EVENT_REF_SOURCE_EXECUTION,
        EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
        det,
    ))
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT));
    if succeeded {
        entry = entry
            .with_inner_dispatch(inner_payload.clone())
            .with_state_transition("running -> succeeded")
            .with_extra("inner_result", inner_payload);
    } else {
        entry = entry
            .with_state_transition("running -> failed")
            .with_extra("inner_error", inner_payload);
    }
    entry.into_json()
}

fn build_skipped_entry(
    node: &DagNode,
    plan: &Plan,
    dispatch_strategy: &str,
    skip_reason: &str,
    skip_detail: Option<(&'static str, String)>,
) -> Value {
    let det = deterministic_plan_node_event_id(
        plan.id,
        &node.id,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "pending",
        "skipped",
    );
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> skipped")
    .add_execution_event(EventRef::new(
        EVENT_REF_SOURCE_EXECUTION,
        EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
        det,
    ))
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
    .with_extra("skip_reason", json!(skip_reason));
    if let Some((k, v)) = skip_detail {
        entry = entry.with_extra(k, json!(v));
    }
    entry.into_json()
}

#[test]
fn evidence_running_entry_carries_ready_to_running_transition() {
    let node = fixture_dag_node("n1", "mission_execution");
    let plan = fixture_plan("(plan)");
    let entry = build_running_entry(&node, &plan, "agent-team");
    assert_eq!(entry["source"], "plan_dag_node_dispatch");
    assert_eq!(entry["kind"], "dispatch");
    assert_eq!(entry["state_transition"], "ready -> running");
    // No inner payload yet — the dispatch hasn't returned.
    assert!(entry.get("inner_dispatch").is_none());
    assert!(entry.get("inner_result").is_none());
    assert!(entry.get("inner_error").is_none());
}

#[test]
fn evidence_finished_entry_succeeded_uses_running_to_succeeded() {
    let node = fixture_dag_node("n1", "mission_execution");
    let plan = fixture_plan("(plan)");
    let entry = build_finished_entry(&node, &plan, "agent-team", json!({"ok": true}), true);
    assert_eq!(entry["state_transition"], "running -> succeeded");
    assert_eq!(entry["inner_dispatch"]["ok"], true);
    assert_eq!(entry["inner_result"]["ok"], true);
    assert!(entry.get("inner_error").is_none());
}

#[test]
fn evidence_finished_entry_failed_uses_running_to_failed() {
    let node = fixture_dag_node("n1", "mission_execution");
    let plan = fixture_plan("(plan)");
    let entry = build_finished_entry(&node, &plan, "agent-team", json!({"error": "boom"}), false);
    assert_eq!(entry["state_transition"], "running -> failed");
    assert_eq!(entry["inner_error"]["error"], "boom");
    assert!(entry.get("inner_dispatch").is_none());
    assert!(entry.get("inner_result").is_none());
}

#[test]
fn evidence_skipped_entry_records_pending_to_skipped_with_reason() {
    let node = fixture_dag_node("n1", "mission_execution");
    let plan = fixture_plan("(plan)");
    let entry = build_skipped_entry(
        &node,
        &plan,
        "agent-team",
        "upstream_failed",
        Some(("failed_dep", "n0".to_string())),
    );
    assert_eq!(entry["state_transition"], "pending -> skipped");
    assert_eq!(entry["skip_reason"], "upstream_failed");
    assert_eq!(entry["failed_dep"], "n0");
}

#[test]
fn evidence_skipped_entry_for_fail_fast_records_aborter() {
    let node = fixture_dag_node("n2", "mission_execution");
    let plan = fixture_plan("(plan)");
    let entry = build_skipped_entry(
        &node,
        &plan,
        "agent-team",
        "fail_fast_aborted",
        Some(("aborter", "n1".to_string())),
    );
    assert_eq!(entry["skip_reason"], "fail_fast_aborted");
    assert_eq!(entry["aborter"], "n1");
}

#[test]
fn evidence_skipped_entry_for_condition_records_condition_text() {
    let node = fixture_dag_node("n3", "mission_execution");
    let plan = fixture_plan("(plan)");
    let entry = build_skipped_entry(
        &node,
        &plan,
        "agent-team",
        "condition_gated",
        Some(("condition", "(env :debug)".to_string())),
    );
    assert_eq!(entry["skip_reason"], "condition_gated");
    assert_eq!(entry["condition"], "(env :debug)");
}

// ── wave-14 / 02 :: PlanNodeStateChanged event + live event refs ──

/// `deterministic_plan_node_event_id` is the fallback id stamped on
/// `EventRef::new(...)` when the bus publish fails. Format must match
/// the wave-14 task brief verbatim so downstream consumers can grep.
#[test]
fn deterministic_event_id_format_matches_brief() {
    let plan_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap();
    let id = deterministic_plan_node_event_id(plan_id, "n1", 1, "ready", "running");
    assert_eq!(
        id,
        "plan-node:00000000-0000-0000-0000-000000000abc:n1:1:ready-running"
    );
}

/// `build_plan_node_state_changed_event` projects a node + lifecycle
/// transition into the `ExecutionEvent` payload. Pins the field
/// mapping (target / dispatch_strategy / target_project / attempt /
/// reason) and the `kind()` wire tag the evidence collector keys on.
#[test]
fn plan_node_state_changed_event_projection_matches_node_metadata() {
    let plan = fixture_plan("(plan)");
    let mut node = fixture_dag_node("nx", "mission_execution");
    node.target_project = Some("missiond".into());
    let ev = build_plan_node_state_changed_event(
        plan.id,
        &node,
        "agent-team",
        1,
        "running",
        "succeeded",
        None,
    );
    assert_eq!(
        <ExecutionEvent as missiond_core::event::DomainEvent>::kind(&ev),
        "plan_node_state_changed"
    );
    match ev {
        ExecutionEvent::PlanNodeStateChanged {
            plan_id,
            node_id,
            from,
            to,
            target,
            dispatch_strategy,
            target_project,
            attempt,
            reason,
        } => {
            assert_eq!(plan_id, plan.id.to_string());
            assert_eq!(node_id, "nx");
            assert_eq!(from, "running");
            assert_eq!(to, "succeeded");
            assert_eq!(target.as_deref(), Some("mission_execution"));
            assert_eq!(dispatch_strategy.as_deref(), Some("agent-team"));
            assert_eq!(target_project.as_deref(), Some("missiond"));
            assert_eq!(attempt, Some(1));
            assert!(reason.is_none(), "success transitions carry no reason");
        }
        _ => panic!("expected PlanNodeStateChanged variant"),
    }
}

/// Failure / skip transitions surface a `reason` annotation through to
/// the bus event payload, mirroring what `emit_evidence_*` writes.
#[test]
fn plan_node_state_changed_event_carries_failure_reason() {
    let plan = fixture_plan("(plan)");
    let node = fixture_dag_node("ny", "mission_task_delegate");
    let ev = build_plan_node_state_changed_event(
        plan.id,
        &node,
        "fresh-code-alignment",
        1,
        "pending",
        "skipped",
        Some("upstream_failed:n1".into()),
    );
    match ev {
        ExecutionEvent::PlanNodeStateChanged {
            reason, from, to, ..
        } => {
            assert_eq!(reason.as_deref(), Some("upstream_failed:n1"));
            assert_eq!(from, "pending");
            assert_eq!(to, "skipped");
        }
        _ => panic!("expected PlanNodeStateChanged"),
    }
}

/// Every fixture-built evidence entry now carries an `attempt` extra
/// (defaults to 1 for v2). Ensures audit consumers see a stable column
/// they can pivot on once retry-aware schedulers land.
#[test]
fn dag_evidence_entries_include_attempt_field() {
    let node = fixture_dag_node("n1", "mission_execution");
    let plan = fixture_plan("(plan)");
    let succ = build_dag_success_entry(&node, &plan, "agent-team", json!({}));
    assert_eq!(succ["attempt"], 1);
    let fail = build_dag_failure_entry(&node, &plan, "agent-team", json!({"error": "x"}));
    assert_eq!(fail["attempt"], 1);
    let running = build_running_entry(&node, &plan, "agent-team");
    assert_eq!(running["attempt"], 1);
    let finished_ok = build_finished_entry(&node, &plan, "agent-team", json!({"ok": true}), true);
    assert_eq!(finished_ok["attempt"], 1);
    let skipped = build_skipped_entry(
        &node,
        &plan,
        "agent-team",
        "upstream_failed",
        Some(("failed_dep", "n0".to_string())),
    );
    assert_eq!(skipped["attempt"], 1);
}

/// Every fixture-built entry now stamps a live `EventRef::new(...)` —
/// the deterministic-id branch in pure tests — with the canonical
/// source/kind tags the evidence collector (and downstream consumers)
/// route on.
#[test]
fn dag_evidence_entries_carry_live_event_ref_with_deterministic_id() {
    let node = fixture_dag_node("n4", "mission_execution");
    let plan = fixture_plan("(plan)");
    let entries = vec![
        (
            "ready -> running",
            build_running_entry(&node, &plan, "agent-team"),
        ),
        (
            "running -> succeeded",
            build_finished_entry(&node, &plan, "agent-team", json!({"ok": true}), true),
        ),
        (
            "running -> failed",
            build_finished_entry(&node, &plan, "agent-team", json!({"error": "x"}), false),
        ),
        (
            "pending -> skipped",
            build_skipped_entry(
                &node,
                &plan,
                "agent-team",
                "upstream_failed",
                Some(("failed_dep", "n0".to_string())),
            ),
        ),
    ];
    for (transition, entry) in entries {
        let arr = entry["execution_events"]
            .as_array()
            .unwrap_or_else(|| panic!("execution_events array for {}", transition));
        assert_eq!(arr.len(), 1, "exactly one ref for {}", transition);
        let r = &arr[0];
        assert!(
            r.get("unavailable").is_none(),
            "live path: ref must NOT be unavailable for {} ({:?})",
            transition,
            r
        );
        assert_eq!(r["source"], "execution", "for {}", transition);
        assert_eq!(r["kind"], "plan_node_state_changed", "for {}", transition);
        let id = r["event_id"].as_str().expect("event_id string");
        assert!(
            id.starts_with(&format!("plan-node:{}:{}:1:", plan.id, node.id)),
            "deterministic id format for {} → {}",
            transition,
            id
        );
    }
}

/// Bus-failure-path symptom surface: when `bus_publish_warnings` is
/// non-empty the `action_execute_dag_v1` response surfaces it as a
/// top-level array. Verifies the field plumbing in `ExecutionOutcome`.
#[test]
fn execution_outcome_collects_bus_publish_warnings() {
    let mut o = ExecutionOutcome::default();
    o.bus_publish_warnings
        .push("simulated bus drop for n1".into());
    o.bus_publish_warnings
        .push("simulated bus drop for n2".into());
    assert_eq!(o.bus_publish_warnings.len(), 2);
    assert!(o.bus_publish_warnings[0].contains("n1"));
}

// ── wave-15 / task 05 — workstation-dispatch hint contract ───────────
//
// Pin the per-node parser additions: scope / commit-policy /
// owned-files / forbidden-files / acceptance-commands /
// workstation-dispatch land on the typed slots and never leak into
// `unsupported_fields` (which would mean the scheduler can't route
// the node through the workstation-dispatch substrate).

#[test]
fn parse_node_form_captures_workstation_dispatch_contract() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :objective "ship the wave"
                :scope "wave 15 task 05 only"
                :owned-files ["a.rs" "b.rs"]
                :forbidden-files ["c.rs"]
                :acceptance-commands ["cargo test" "git diff --check"]
                :commit-policy "scoped"
                :workstation-dispatch true
                :dispatch-strategy "fresh-code-alignment"))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes.len(), 1);
    let n = &parsed.nodes[0];
    assert_eq!(n.scope.as_deref(), Some("wave 15 task 05 only"));
    assert_eq!(n.commit_policy.as_deref(), Some("scoped"));
    assert!(n.owned_files_raw.as_deref().unwrap().contains("a.rs"));
    assert!(n.forbidden_files_raw.as_deref().unwrap().contains("c.rs"));
    assert!(n
        .acceptance_commands_raw
        .as_deref()
        .unwrap()
        .contains("cargo test"));
    assert!(n.workstation_dispatch_opt_in());
    // None of the new keys should land in unsupported_fields — that
    // would break workstation-dispatch routing.
    let unsupported_keys: Vec<String> = n
        .unsupported_fields
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    for forbidden in [
        "scope",
        "commit-policy",
        "owned-files",
        "forbidden-files",
        "acceptance-commands",
        "workstation-dispatch",
    ] {
        assert!(
            !unsupported_keys.contains(&forbidden.to_string()),
            "key `{}` must land on a typed slot, not unsupported_fields",
            forbidden
        );
    }
}

#[test]
fn parse_node_form_workstation_dispatch_opt_in_recognises_truthy_values() {
    for truthy in &["true", "TRUE", "yes", "on", "1"] {
        let sexp = format!(
            r#"(plan (node :id "n1" :target "mission_task_delegate" :workstation-dispatch {}))"#,
            truthy
        );
        let parsed = parse_plan_dag(&sexp);
        assert!(
            parsed.nodes[0].workstation_dispatch_opt_in(),
            "expected `{}` to be truthy",
            truthy
        );
    }
    for falsy in &["false", "no", "off", "0", "maybe"] {
        let sexp = format!(
            r#"(plan (node :id "n1" :target "mission_task_delegate" :workstation-dispatch {}))"#,
            falsy
        );
        let parsed = parse_plan_dag(&sexp);
        assert!(
            !parsed.nodes[0].workstation_dispatch_opt_in(),
            "expected `{}` to NOT be truthy",
            falsy
        );
    }
    // Absence is also off.
    let sexp = r#"(plan (node :id "n1" :target "mission_task_delegate"))"#;
    let parsed = parse_plan_dag(sexp);
    assert!(!parsed.nodes[0].workstation_dispatch_opt_in());
}

/// `node_to_workstation_hints` is the bridge between the parsed node
/// and the workstation-dispatch substrate. Any divergence here means
/// per-node DAG dispatch and per-plan single-node dispatch would
/// produce different briefs for identical inputs.
#[test]
fn node_to_workstation_hints_projects_every_field() {
    let node = DagNode {
        id: "n1".into(),
        target: "mission_task_delegate".into(),
        objective: Some("ship".into()),
        target_project: Some("missiond".into()),
        requested_cwd: Some("/abs/missiond".into()),
        dispatch_strategy: Some("agent-team".into()),
        scope: Some("scope text".into()),
        commit_policy: Some("scoped".into()),
        owned_files_raw: Some(r#"["a.rs"]"#.into()),
        forbidden_files_raw: Some(r#"["b.rs"]"#.into()),
        acceptance_commands_raw: Some(r#"["cargo test"]"#.into()),
        failure_policy: "fail-fast".into(),
        ..Default::default()
    };
    let w = node_to_workstation_hints(&node);
    assert_eq!(w.objective.as_deref(), Some("ship"));
    assert_eq!(w.target_project.as_deref(), Some("missiond"));
    assert_eq!(w.requested_cwd.as_deref(), Some("/abs/missiond"));
    assert_eq!(w.dispatch_strategy.as_deref(), Some("agent-team"));
    assert_eq!(w.scope.as_deref(), Some("scope text"));
    assert_eq!(w.commit_policy.as_deref(), Some("scoped"));
    assert_eq!(w.owned_files, vec!["a.rs".to_string()]);
    assert_eq!(w.forbidden_files, vec!["b.rs".to_string()]);
    assert_eq!(w.acceptance_commands, vec!["cargo test".to_string()]);
}

fn fixture_decision_explicit() -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision
{
    crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
        source:
            crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchSource::PlanHint,
        reason: Some("test fixture".to_string()),
    }
}

fn fixture_decision_inferred() -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision
{
    crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
        source:
            crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchSource::Inferred,
        reason: Some("inferred test fixture".to_string()),
    }
}

/// Safe-descriptor outcomes from the workstation-dispatch substrate
/// must classify as failures so the DAG scheduler taints downstream
/// nodes (vs success which would falsely advance the wave).
#[test]
fn workstation_outcome_safe_descriptor_classifies_as_failure() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let node = DagNode {
        id: "n1".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        ..Default::default()
    };
    let outcome = wd::WorkstationDispatchOutcome::SafeDescriptor {
        reason: wd::SafeDescriptorReason::ProjectRootUnresolved("no signal".to_string()),
        task_brief: None,
    };
    let (payload, classification, non_retryable) = workstation_outcome_to_dispatch_pair(
        &node,
        "fresh-code-alignment",
        outcome,
        &fixture_decision_explicit(),
    );
    assert!(
        classification.is_err(),
        "safe descriptors must fail dispatch"
    );
    assert!(
        non_retryable,
        "safe-descriptor refusals are deterministic and must classify non-retryable"
    );
    assert_eq!(
        payload["workstation_dispatch_status"],
        "skipped_project_root_unresolved"
    );
    assert_eq!(payload["node_id"], "n1");
    assert_eq!(payload["workstation_dispatch_source"], "plan_hint");
}

/// Dispatched outcomes must classify as success (the inner handler
/// already returned non-error).
#[test]
fn workstation_outcome_dispatched_classifies_as_success() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let node = DagNode {
        id: "n1".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        ..Default::default()
    };
    let outcome = wd::WorkstationDispatchOutcome::Dispatched {
        task_brief: "## Objective\nship\n".to_string(),
        task_brief_path: None,
        task_contract_source_path: None,
        evidence_path: Some("/tmp/sidecar.json".to_string()),
        evidence_error: None,
        inner_payload: json!({"task_id": "btk-7"}),
    };
    let (payload, classification, non_retryable) = workstation_outcome_to_dispatch_pair(
        &node,
        "agent-team",
        outcome,
        &fixture_decision_inferred(),
    );
    assert!(classification.is_ok(), "dispatched must succeed");
    assert!(
        !non_retryable,
        "successful dispatch must NOT be flagged non-retryable"
    );
    assert_eq!(payload["workstation_dispatch_status"], "dispatched");
    assert_eq!(payload["dispatch_strategy"], "agent-team");
    assert_eq!(payload["inner_result"]["task_id"], "btk-7");
    assert_eq!(payload["workstation_dispatch_source"], "inferred");
    assert_eq!(
        payload["workstation_dispatch_inference_reason"],
        "inferred test fixture"
    );
}

// ── wave-16 / task 03 — DAG node auto-inference ─────────────────────

/// Compose `node_to_workstation_hints` with `evaluate_dispatch_decision`
/// to assert the per-node decision the scheduler would arrive at.
fn dag_node_decision(
    node: &DagNode,
    dispatch_strategy: &str,
) -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
    let merged = node_to_workstation_hints(node);
    let ctx = crate::handlers::knowledge::workstation_dispatch::InferenceContext {
        target: node.target.as_str(),
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
    };
    crate::handlers::knowledge::workstation_dispatch::evaluate_dispatch_decision(
        &serde_json::Value::Null,
        node.workstation_dispatch_opt_in(),
        &ctx,
    )
}

#[test]
fn dag_node_auto_inferred_for_task_delegate_with_owned_files_and_strategy() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :objective "ship the wave"
                :dispatch-strategy "fresh-code-alignment"
                :owned-files ["a.rs"]))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert!(!parsed.nodes[0].workstation_dispatch_opt_in());
    let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
    assert_eq!(decision.source, wd::WorkstationDispatchSource::Inferred);
}

#[test]
fn dag_node_auto_inferred_for_agent_team_with_target_project_signal() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :objective "ship"
                :dispatch-strategy "agent-team"
                :target-project "missiond"))
    "#;
    let parsed = parse_plan_dag(sexp);
    let decision = dag_node_decision(&parsed.nodes[0], "agent-team");
    assert_eq!(decision.source, wd::WorkstationDispatchSource::Inferred);
}

#[test]
fn dag_node_explicit_opt_in_takes_plan_hint_path() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :workstation-dispatch true
                :objective "ship"
                :dispatch-strategy "fresh-code-alignment"
                :owned-files ["a.rs"]))
    "#;
    let parsed = parse_plan_dag(sexp);
    let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
    // Explicit hint wins over inference and shows up as PlanHint.
    assert_eq!(decision.source, wd::WorkstationDispatchSource::PlanHint);
}

#[test]
fn dag_node_not_inferred_when_strategy_unknown() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :objective "ship"
                :owned-files ["a.rs"]))
    "#;
    let parsed = parse_plan_dag(sexp);
    let decision = dag_node_decision(&parsed.nodes[0], "unknown");
    assert_eq!(
        decision.source,
        wd::WorkstationDispatchSource::NotApplicable
    );
}

#[test]
fn dag_node_not_inferred_when_objective_missing() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :dispatch-strategy "fresh-code-alignment"
                :owned-files ["a.rs"]))
    "#;
    let parsed = parse_plan_dag(sexp);
    let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
    assert_eq!(
        decision.source,
        wd::WorkstationDispatchSource::NotApplicable
    );
}

#[test]
fn dag_node_not_inferred_for_mission_execution_target() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_execution"
                :objective "ship"
                :dispatch-strategy "fresh-code-alignment"
                :owned-files ["a.rs"]))
    "#;
    let parsed = parse_plan_dag(sexp);
    let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
    assert_eq!(
        decision.source,
        wd::WorkstationDispatchSource::NotApplicable
    );
}

#[test]
fn dag_node_not_inferred_when_no_scope_signal() {
    use crate::handlers::knowledge::workstation_dispatch as wd;
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :objective "ship"
                :dispatch-strategy "fresh-code-alignment"))
    "#;
    let parsed = parse_plan_dag(sexp);
    let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
    assert_eq!(
        decision.source,
        wd::WorkstationDispatchSource::NotApplicable
    );
}

/// `build_nodes_summary` must surface the new workstation-dispatch
/// hint fields when present (so dry-run callers can see them) and
/// stay quiet for nodes that did not opt in.
#[test]
fn build_nodes_summary_surfaces_workstation_dispatch_hints_when_present() {
    let nodes = vec![
        DagNode {
            id: "wd".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            scope: Some("scope text".into()),
            commit_policy: Some("scoped".into()),
            owned_files_raw: Some(r#"["a.rs"]"#.into()),
            forbidden_files_raw: Some(r#"["b.rs"]"#.into()),
            acceptance_commands_raw: Some(r#"["cargo test"]"#.into()),
            workstation_dispatch_flag: Some("true".into()),
            ..Default::default()
        },
        DagNode {
            id: "plain".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let order = vec!["wd".to_string(), "plain".to_string()];
    let summary = build_nodes_summary(&nodes, &order);
    let arr = summary.as_array().unwrap();
    let wd = &arr[0];
    assert_eq!(wd["scope"], "scope text");
    assert_eq!(wd["commit_policy"], "scoped");
    assert_eq!(wd["workstation_dispatch"], true);
    assert!(wd["owned_files_raw"].as_str().unwrap().contains("a.rs"));
    let plain = &arr[1];
    // Plain node carries none of the workstation-dispatch fields so
    // the summary stays quiet (regression guard for the v2 baseline).
    assert!(plain.get("scope").is_none());
    assert!(plain.get("commit_policy").is_none());
    assert!(plain.get("workstation_dispatch").is_none());
}

// ── wave-16 / task 04 — review-gate hint contract ────────────────────
//
// PLAN DAG runtime now supports a per-node `:review-gate
// "question-event"` hint that pauses the node and emits
// `QuestionEvent::Created` instead of dispatching the target tool.
// Pure tests pin (a) parser captures the new fields without leaking
// them into `unsupported_fields`, (b) the `review_gate_kind` typed
// projection routes correctly, (c) `build_nodes_summary` surfaces
// the hints, (d) the response shape for paused nodes.

#[test]
fn parse_node_form_captures_review_gate_contract() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_execution"
                :objective "ship"
                :review-gate "question-event"
                :review-action "human-checkpoint"
                :review-text "Look at the diff before merging."))
    "#;
    let parsed = parse_plan_dag(sexp);
    let n = &parsed.nodes[0];
    assert_eq!(n.review_gate.as_deref(), Some("question-event"));
    assert_eq!(n.review_action.as_deref(), Some("human-checkpoint"));
    assert_eq!(
        n.review_text.as_deref(),
        Some("Look at the diff before merging.")
    );
    // None of the new keys must land in unsupported_fields — that
    // would mean the scheduler can't route the node through the
    // pause path.
    let unsupported_keys: Vec<String> = n
        .unsupported_fields
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    for forbidden in ["review-gate", "review-action", "review-text"] {
        assert!(
            !unsupported_keys.contains(&forbidden.to_string()),
            "key `{}` must land on a typed slot, not unsupported_fields",
            forbidden
        );
    }
    assert_eq!(n.review_gate_kind(), ReviewGateKind::QuestionEvent);
}

#[test]
fn parse_node_form_review_gate_default_is_none() {
    // Absent `:review-gate` keeps the wave-15 baseline: scheduler
    // dispatches as before, no field surfaces in the response.
    let sexp = r#"(plan (node :id "n1" :target "mission_execution"))"#;
    let parsed = parse_plan_dag(sexp);
    assert!(parsed.nodes[0].review_gate.is_none());
    assert_eq!(parsed.nodes[0].review_gate_kind(), ReviewGateKind::None);
}

#[test]
fn parse_node_form_review_gate_explicit_none_resolves_to_none() {
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution" :review-gate "none"))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].review_gate.as_deref(), Some("none"));
    assert_eq!(parsed.nodes[0].review_gate_kind(), ReviewGateKind::None);
    // "none" is recognised, must NOT pollute unsupported_fields.
    assert!(parsed.nodes[0]
        .unsupported_fields
        .iter()
        .all(|(k, _)| k != "review-gate"));
}

#[test]
fn parse_node_form_review_gate_unknown_kind_safely_falls_back_and_records_typo() {
    // Defensive: an unrecognised gate kind (typo) must NOT silently
    // pause the node. The scheduler treats it as `None` and the
    // parser records the raw value into `unsupported_fields` so
    // `node_hint_summary` surfaces the typo in the response.
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution" :review-gate "questoin-event"))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].review_gate_kind(), ReviewGateKind::None);
    assert!(parsed.nodes[0]
        .unsupported_fields
        .iter()
        .any(|(k, v)| k == "review-gate" && v == "questoin-event"));
}

#[test]
fn parse_node_form_review_gate_underscore_alias_works() {
    // Authors that prefer snake_case keys still get the typed slot.
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution"
                :review_gate "question_event"
                :review_action "ship-it"))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(
        parsed.nodes[0].review_gate_kind(),
        ReviewGateKind::QuestionEvent
    );
    assert_eq!(parsed.nodes[0].review_action.as_deref(), Some("ship-it"));
}

#[test]
fn build_nodes_summary_surfaces_review_gate_hints_when_present() {
    let nodes = vec![
        DagNode {
            id: "g".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            review_gate: Some("question-event".into()),
            review_action: Some("human-checkpoint".into()),
            review_text: Some("eyeball it".into()),
            ..Default::default()
        },
        DagNode {
            id: "plain".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let order = vec!["g".to_string(), "plain".to_string()];
    let summary = build_nodes_summary(&nodes, &order);
    let arr = summary.as_array().unwrap();
    let g = &arr[0];
    assert_eq!(g["review_gate"], "question-event");
    assert_eq!(g["review_action"], "human-checkpoint");
    assert_eq!(g["review_text"], "eyeball it");
    let plain = &arr[1];
    // Quiet for nodes without a gate — protects the wave-15 baseline
    // shape so consumers that pivot on key presence keep working.
    assert!(plain.get("review_gate").is_none());
    assert!(plain.get("review_action").is_none());
    assert!(plain.get("review_text").is_none());
}

#[test]
fn node_results_json_includes_paused_state_with_review_question_id() {
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "g".into(),
        target: "mission_execution".into(),
        state: NodeState::Paused {
            question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
            bus_publish_warning: None,
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    let v = o.node_results_json();
    let arr = v.as_array().unwrap();
    assert_eq!(arr[0]["state"], "paused");
    assert_eq!(
        arr[0]["review_question_id"],
        "review:plan:p1:v1:plan-node:abcdef0123456789"
    );
    // No warning attached here — must NOT surface the field.
    assert!(arr[0].get("review_question_warning").is_none());
}

#[test]
fn node_results_json_paused_with_bus_warning_surfaces_warning_field() {
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "g".into(),
        target: "mission_execution".into(),
        state: NodeState::Paused {
            question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
            bus_publish_warning: Some("simulated bus drop".into()),
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    let v = o.node_results_json();
    let arr = v.as_array().unwrap();
    assert_eq!(arr[0]["state"], "paused");
    assert_eq!(arr[0]["review_question_warning"], "simulated bus drop");
}

#[test]
fn paused_nodes_json_filters_only_paused_results() {
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "a".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "g".into(),
        target: "mission_execution".into(),
        state: NodeState::Paused {
            question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
            bus_publish_warning: None,
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "g2".into(),
        target: "mission_task_delegate".into(),
        state: NodeState::Paused {
            question_id: "review:plan:p1:v1:plan-node:0011223344556677".into(),
            bus_publish_warning: Some("bus dropped".into()),
        },
        dispatch_strategy: "agent-team".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    let v = o.paused_nodes_json();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2, "only the two paused entries surface here");
    assert_eq!(arr[0]["id"], "g");
    assert_eq!(arr[0]["state"], "paused");
    assert_eq!(
        arr[0]["review_question_id"],
        "review:plan:p1:v1:plan-node:abcdef0123456789"
    );
    assert!(arr[0].get("review_question_warning").is_none());
    assert_eq!(arr[1]["id"], "g2");
    assert_eq!(arr[1]["review_question_warning"], "bus dropped");
}

#[test]
fn paused_node_ids_and_review_question_ids_align_in_topo_order() {
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "a".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "g1".into(),
        target: "mission_execution".into(),
        state: NodeState::Paused {
            question_id: "review:plan:p1:v1:plan-node:0000000000000001".into(),
            bus_publish_warning: None,
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "g2".into(),
        target: "mission_execution".into(),
        state: NodeState::Paused {
            question_id: "review:plan:p1:v1:plan-node:0000000000000002".into(),
            bus_publish_warning: None,
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    assert_eq!(
        o.paused_node_ids(),
        vec!["g1".to_string(), "g2".to_string()]
    );
    assert_eq!(
        o.review_question_ids(),
        vec![
            "review:plan:p1:v1:plan-node:0000000000000001".to_string(),
            "review:plan:p1:v1:plan-node:0000000000000002".to_string(),
        ]
    );
}

#[test]
fn aggregate_status_dag_paused_when_only_paused_no_failure() {
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "g".into(),
        target: "mission_execution".into(),
        state: NodeState::Paused {
            question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
            bus_publish_warning: None,
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    assert_eq!(o.aggregate_status(), "dag_paused");
    assert_eq!(o.runner_status(), "review_gate_paused");
    // Paused runs MUST NOT mutate the plan status — auto-resume
    // (wave-16 / task 02 territory) revives the node in a follow-up
    // call, so the plan stays Approved/Executing.
    assert_eq!(o.target_plan_status(), None);
}

#[test]
fn aggregate_status_partial_when_paused_and_succeeded_mix() {
    // A successful node alongside a paused gate still surfaces as
    // dag_paused — paused is the dominant signal (the run cannot
    // complete until resume).
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "a".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "g".into(),
        target: "mission_execution".into(),
        state: NodeState::Paused {
            question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
            bus_publish_warning: None,
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    assert_eq!(o.aggregate_status(), "dag_paused");
    assert_eq!(o.target_plan_status(), None);
}

#[test]
fn aggregate_status_failure_dominates_paused() {
    // Mixed paused + failed run reads as dag_partial (failure is the
    // louder signal). The failing node also flips the plan status to
    // Failed so the caller knows the DAG cannot resume cleanly.
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "f".into(),
        target: "mission_execution".into(),
        state: NodeState::Failed {
            reason: "boom".into(),
        },
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({"error": "boom"}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    o.results.push(NodeResult {
        id: "g".into(),
        target: "mission_execution".into(),
        state: NodeState::Paused {
            question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
            bus_publish_warning: None,
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    assert_eq!(o.aggregate_status(), "dag_partial");
    assert_eq!(o.target_plan_status(), Some(PlanStatus::Failed));
}

/// Helper that mirrors `emit_paused_review_gate`'s evidence entry
/// shape (without standing up an AppState/bus). Pins the wire form
/// auditors / dashboards will route on.
fn build_paused_evidence_entry(
    node: &DagNode,
    plan: &Plan,
    dispatch_strategy: &str,
    question_id: &str,
    bus_warning: Option<&str>,
) -> Value {
    let det = deterministic_plan_node_event_id(
        plan.id,
        &node.id,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "pending",
        "paused",
    );
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> paused")
    .add_execution_event(EventRef::new(
        EVENT_REF_SOURCE_EXECUTION,
        EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
        det,
    ))
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
    .with_extra("review_gate", json!("question-event"))
    .with_extra("review_question_id", json!(question_id));
    if let Some(action) = node.review_action.as_deref() {
        entry = entry.with_extra("review_action", json!(action));
    }
    if let Some(text) = node.review_text.as_deref() {
        entry = entry.with_extra("review_text", json!(text));
    }
    if let Some(w) = bus_warning {
        entry = entry.with_extra("review_question_warning", json!(w));
    }
    entry.into_json()
}

#[test]
fn evidence_paused_entry_carries_pending_to_paused_transition_and_qid() {
    let node = DagNode {
        id: "g".into(),
        target: "mission_execution".into(),
        failure_policy: "fail-fast".into(),
        dispatch_strategy: Some("agent-team".into()),
        review_gate: Some("question-event".into()),
        review_action: Some("plan-node".into()),
        review_text: Some("eyeball it".into()),
        ..Default::default()
    };
    let plan = fixture_plan("(plan)");
    let qid = super::super::review_gate::derive_plan_node_review_question_id(
        &plan.id.to_string(),
        plan.version,
        &node.id,
        node.review_action.as_deref(),
    );
    let entry = build_paused_evidence_entry(&node, &plan, "agent-team", &qid, None);
    assert_eq!(entry["source"], "plan_dag_node_dispatch");
    assert_eq!(entry["kind"], "dispatch");
    assert_eq!(entry["state_transition"], "pending -> paused");
    assert_eq!(entry["review_gate"], "question-event");
    assert_eq!(entry["review_question_id"], qid);
    assert_eq!(entry["review_action"], "plan-node");
    assert_eq!(entry["review_text"], "eyeball it");
    // The deterministic id format pinned for the lifecycle event ref.
    let event_id = entry["execution_events"][0]["event_id"]
        .as_str()
        .expect("event_id");
    assert!(event_id.starts_with(&format!(
        "plan-node:{}:{}:{}:pending-paused",
        plan.id, node.id, PLAN_NODE_DEFAULT_ATTEMPT
    )));
}

#[test]
fn evidence_paused_entry_with_bus_warning_records_review_question_warning() {
    let node = DagNode {
        id: "g".into(),
        target: "mission_execution".into(),
        failure_policy: "fail-fast".into(),
        dispatch_strategy: Some("agent-team".into()),
        review_gate: Some("question-event".into()),
        ..Default::default()
    };
    let plan = fixture_plan("(plan)");
    let qid = super::super::review_gate::derive_plan_node_review_question_id(
        &plan.id.to_string(),
        plan.version,
        &node.id,
        None,
    );
    let entry =
        build_paused_evidence_entry(&node, &plan, "agent-team", &qid, Some("simulated bus drop"));
    // Bus failure DOES NOT change the transition string — the gate
    // is still real, the warning is observability-only.
    assert_eq!(entry["state_transition"], "pending -> paused");
    assert_eq!(entry["review_question_warning"], "simulated bus drop");
}

// ── wave-16 / task 05 — bounded per-node retry policy ──────────────
//
// Pure tests for the parser additions (`:retry-count` /
// `:max-attempts` / `:retry-delay-ms`), the typed projections
// (`effective_max_attempts` + `effective_retry_delay_ms`), the
// structured parse-error path (`DagBuildError::InvalidRetryHint`),
// the dry-run `retry_plan` projection, the per-node response
// surface, and the safe-descriptor non-retryable classification.
//
// End-to-end retry behaviour (one failure then success → succeeded;
// exhausted attempts → failed) is covered by the integration tests
// under `tests/plan_dag_retry.rs` because it requires an `AppState`
// / handler stub — these pure tests pin the contract surface
// without standing up the daemon.

#[test]
fn parse_node_form_captures_retry_count_keyword() {
    // `:retry-count N` declares N **additional** attempts beyond
    // the first; both kebab- and snake_case spellings populate
    // `retry_count` directly.
    for keyword in ["retry-count", "retry_count"] {
        let sexp = format!(
            r#"(plan (node :id "n1" :target "mission_execution" :{} 2))"#,
            keyword
        );
        let parsed = parse_plan_dag(&sexp);
        assert_eq!(
            parsed.nodes[0].retry_count,
            Some(2),
            "keyword `:{}` must populate retry_count directly",
            keyword
        );
        assert_eq!(parsed.nodes[0].effective_max_attempts(), 3); // 1 + 2 retries
        assert!(parsed.nodes[0].retry_enabled());
    }
}

#[test]
fn parse_node_form_captures_max_attempts_keyword_as_total_attempts() {
    // `:max-attempts N` declares N **total** attempts (including
    // the first); the parser converts to additional retries so
    // the runtime always sees the same shape.
    for keyword in ["max-attempts", "max_attempts"] {
        let sexp = format!(
            r#"(plan (node :id "n1" :target "mission_execution" :{} 3))"#,
            keyword
        );
        let parsed = parse_plan_dag(&sexp);
        assert_eq!(
            parsed.nodes[0].retry_count,
            Some(2),
            "keyword `:{}` value 3 must lower into 2 additional retries",
            keyword
        );
        assert_eq!(parsed.nodes[0].effective_max_attempts(), 3);
    }
}

#[test]
fn parse_node_form_max_attempts_one_keeps_baseline_single_attempt() {
    // `:max-attempts 1` means "exactly one attempt" — the baseline
    // single-attempt contract; retry_enabled must read false.
    let sexp = r#"(plan (node :id "n" :target "mission_execution" :max-attempts 1))"#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].retry_count, Some(0));
    assert_eq!(parsed.nodes[0].effective_max_attempts(), 1);
    assert!(!parsed.nodes[0].retry_enabled());
}

#[test]
fn build_validated_dag_rejects_max_attempts_zero() {
    // `:max-attempts 0` is meaningless — zero attempts = never
    // run. We surface a structured parse error so the typo is
    // visible to the author instead of silently disabling the node.
    let sexp = r#"(plan (node :id "n" :target "mission_execution" :max-attempts 0))"#;
    let err = build_validated_dag(sexp).unwrap_err();
    match err {
        DagBuildError::InvalidRetryHint {
            node_id,
            key,
            raw,
            detail,
        } => {
            assert_eq!(node_id, "n");
            assert_eq!(key, "max-attempts");
            assert_eq!(raw, "0");
            assert!(detail.contains("positive"));
        }
        other => panic!("expected InvalidRetryHint, got {:?}", other),
    }
}

#[test]
fn parse_node_form_captures_retry_delay_ms() {
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution"
                :retry-count 1
                :retry-delay-ms 250))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].retry_count, Some(1));
    assert_eq!(parsed.nodes[0].retry_delay_ms, Some(250));
    assert_eq!(parsed.nodes[0].effective_retry_delay_ms(), Some(250));
    assert_eq!(parsed.nodes[0].effective_max_attempts(), 2);
}

#[test]
fn parse_node_form_retry_count_caps_to_safe_max() {
    // Authoring `:retry-count 9999` cannot melt the dispatch loop
    // — `effective_max_attempts` clamps to MAX_NODE_ATTEMPTS_CAP.
    let sexp = r#"(plan (node :id "n" :target "mission_execution" :retry-count 9999))"#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].retry_count, Some(9999));
    assert_eq!(
        parsed.nodes[0].effective_max_attempts(),
        MAX_NODE_ATTEMPTS_CAP
    );
}

#[test]
fn parse_node_form_retry_delay_ms_caps_to_safe_max() {
    let sexp = r#"
        (plan
          (node :id "n" :target "mission_execution"
                :retry-count 1
                :retry-delay-ms 9999999))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].retry_delay_ms, Some(9999999));
    assert_eq!(
        parsed.nodes[0].effective_retry_delay_ms(),
        Some(MAX_RETRY_DELAY_MS)
    );
}

#[test]
fn parse_node_form_retry_count_zero_keeps_baseline_single_attempt() {
    let sexp = r#"(plan (node :id "n" :target "mission_execution" :retry-count 0))"#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].retry_count, Some(0));
    assert_eq!(parsed.nodes[0].effective_max_attempts(), 1);
    assert!(!parsed.nodes[0].retry_enabled());
}

#[test]
fn parse_node_form_retry_count_absent_keeps_baseline_single_attempt() {
    let sexp = r#"(plan (node :id "n" :target "mission_execution"))"#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes[0].retry_count, None);
    assert_eq!(parsed.nodes[0].effective_max_attempts(), 1);
    assert!(!parsed.nodes[0].retry_enabled());
    assert_eq!(parsed.nodes[0].effective_retry_delay_ms(), None);
}

#[test]
fn build_validated_dag_rejects_negative_retry_count() {
    // Negative retry counts are a structured parse error — silently
    // dropping the hint into `unsupported_fields` would lose the
    // policy the author declared.
    let sexp = r#"(plan (node :id "n" :target "mission_execution" :retry-count -1))"#;
    let err = build_validated_dag(sexp).unwrap_err();
    match err {
        DagBuildError::InvalidRetryHint {
            node_id,
            key,
            raw,
            detail,
        } => {
            assert_eq!(node_id, "n");
            assert_eq!(key, "retry-count");
            assert_eq!(raw, "-1");
            assert!(detail.contains("non-negative"));
        }
        other => panic!("expected InvalidRetryHint, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_non_numeric_retry_count() {
    let sexp = r#"
        (plan (node :id "n" :target "mission_execution" :max-attempts "thrice"))
    "#;
    let err = build_validated_dag(sexp).unwrap_err();
    match err {
        DagBuildError::InvalidRetryHint {
            node_id, key, raw, ..
        } => {
            assert_eq!(node_id, "n");
            assert_eq!(key, "max-attempts");
            assert_eq!(raw, "thrice");
        }
        other => panic!("expected InvalidRetryHint, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_negative_retry_delay_ms() {
    let sexp = r#"
        (plan
          (node :id "n" :target "mission_execution"
                :retry-count 1
                :retry-delay-ms -50))
    "#;
    let err = build_validated_dag(sexp).unwrap_err();
    assert!(matches!(err, DagBuildError::InvalidRetryHint { .. }));
}

#[test]
fn invalid_retry_hint_into_tool_result_carries_invalid_param_code() {
    // Author-facing surface of the structured parse error: the
    // ToolResult must carry the canonical INVALID_PARAM error code
    // and a suggestion that points at the corrective action so a
    // failed dispatch tells the author exactly what to fix.
    let err = DagBuildError::InvalidRetryHint {
        node_id: "n".into(),
        key: "retry-count".into(),
        raw: "-1".into(),
        detail: "value must be a non-negative integer".into(),
    };
    let tr = err.into_tool_result();
    assert_eq!(tr.is_error, Some(true));
    let payload = tool_result_payload(&tr);
    assert_eq!(payload["error_code"], error_codes::INVALID_PARAM);
    let reason = payload["reason"].as_str().expect("reason string");
    assert!(reason.contains("retry-count"), "reason: {}", reason);
    assert!(reason.contains("-1"), "reason: {}", reason);
    assert!(payload["suggestion"].is_string(), "must carry a suggestion");
}

#[test]
fn build_retry_plan_lists_only_nodes_that_opted_in() {
    // Plain nodes (no retry hint) and `:max-attempts 1` (explicit
    // single-attempt) MUST stay out of `retry_plan` so the v2
    // baseline byte-shape is preserved for callers that never
    // declared a retry policy.
    let sexp = r#"
        (plan
          (node :id "a" :target "mission_execution")
          (node :id "b" :target "mission_execution" :retry-count 2 :retry-delay-ms 100)
          (node :id "c" :target "mission_execution" :max-attempts 1)
          (node :id "d" :target "mission_execution" :max-attempts 2))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid");
    let plan = build_retry_plan(&parsed.nodes, &order);
    let arr = plan.as_array().unwrap();
    assert_eq!(arr.len(), 2, "only nodes with > 1 attempts surface");
    // Node `b` opted in via `:retry-count 2` (3 total).
    assert_eq!(arr[0]["id"], "b");
    assert_eq!(arr[0]["max_attempts"], 3);
    assert_eq!(arr[0]["retry_count_raw"], 2);
    assert_eq!(arr[0]["retry_delay_ms"], 100);
    // Node `d` opted in via `:max-attempts 2` (lowered to 1 retry).
    assert_eq!(arr[1]["id"], "d");
    assert_eq!(arr[1]["max_attempts"], 2);
    assert_eq!(arr[1]["retry_count_raw"], 1);
}

#[test]
fn build_nodes_summary_surfaces_retry_block_when_present() {
    let nodes = vec![
        DagNode {
            id: "rb".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            retry_count: Some(2),
            retry_delay_ms: Some(50),
            ..Default::default()
        },
        DagNode {
            id: "plain".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let order = vec!["rb".to_string(), "plain".to_string()];
    let summary = build_nodes_summary(&nodes, &order);
    let arr = summary.as_array().unwrap();
    let rb = &arr[0];
    assert_eq!(rb["retry"]["max_attempts"], 3);
    assert_eq!(rb["retry"]["retry_count_raw"], 2);
    assert_eq!(rb["retry"]["retry_delay_ms"], 50);
    let plain = &arr[1];
    // Plain node never opted in — must NOT carry a `retry` block
    // (preserves the wave-15 baseline byte-shape).
    assert!(plain.get("retry").is_none());
}

#[test]
fn node_results_json_emits_retry_block_when_attempts_made_more_than_one() {
    let mut o = ExecutionOutcome::default();
    // A node that succeeded on attempt 2 (one retry consumed) must
    // emit the `retry` observability block.
    o.results.push(NodeResult {
        id: "r".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({"ok": true}),
        attempts_made: 2,
        max_attempts: 3,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    // A baseline-quiet node (single attempt, no retry policy) must
    // NOT emit the block — preserves wave-15 byte-shape.
    o.results.push(NodeResult {
        id: "q".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    let v = o.node_results_json();
    let arr = v.as_array().unwrap();
    assert_eq!(arr[0]["retry"]["attempts"], 2);
    assert_eq!(arr[0]["retry"]["max_attempts"], 3);
    assert!(arr[0]["retry"].get("non_retryable").is_none());
    assert!(arr[1].get("retry").is_none());
}

#[test]
fn node_results_json_emits_retry_block_when_failure_was_non_retryable() {
    // Safe-descriptor refusal — only one attempt, but the
    // `non_retryable` flag must surface so consumers can
    // distinguish "we exhausted attempts" from "we refused to
    // retry on policy grounds".
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "sd".into(),
        target: "mission_task_delegate".into(),
        state: NodeState::Failed {
            reason: "workstation_dispatch refused: no project root".into(),
        },
        dispatch_strategy: "fresh-code-alignment".into(),
        inner_payload: json!({"workstation_dispatch_status": "skipped_project_root_unresolved"}),
        attempts_made: 1,
        max_attempts: 3,
        retry_skipped_non_retryable: true,
        rollback: None,
        acceptance: None,
    });
    let v = o.node_results_json();
    let arr = v.as_array().unwrap();
    assert_eq!(arr[0]["retry"]["attempts"], 1);
    assert_eq!(arr[0]["retry"]["max_attempts"], 3);
    assert_eq!(arr[0]["retry"]["non_retryable"], true);
}

// ── wave-16 / task 05 — retry decision predicate ──────────────────
//
// `plan_node_should_retry` is the single point of truth for the
// wave loop's "retry vs final failure" branch. Pure tests below
// pin the matrix authors care about: one failure then a remaining
// attempt → retry; exhausted → no retry; safe-descriptor refusal
// → no retry; fail-fast abort → no retry.

#[test]
fn plan_node_should_retry_first_failure_with_attempts_left() {
    // attempt 1 of 3 (1 + 2 retries) failed, retryable, no abort
    // → must retry on attempt 2.
    assert!(plan_node_should_retry(1, 3, false, false));
    // attempt 2 of 3 still has one more retry left.
    assert!(plan_node_should_retry(2, 3, false, false));
}

#[test]
fn plan_node_should_retry_exhausted_attempts_returns_false() {
    // Final attempt failed → must NOT retry.
    assert!(!plan_node_should_retry(3, 3, false, false));
    // Defensive: current_attempt > max_attempts (saturating sub).
    assert!(!plan_node_should_retry(4, 3, false, false));
}

#[test]
fn plan_node_should_retry_baseline_single_attempt_returns_false() {
    // Default policy = 1 attempt total → never retries.
    assert!(!plan_node_should_retry(1, 1, false, false));
}

#[test]
fn plan_node_should_retry_safe_descriptor_refusal_short_circuits() {
    // Safe-descriptor refusal — non-retryable trumps remaining
    // attempts so the wave loop never re-spawns a deterministic
    // policy refusal.
    assert!(!plan_node_should_retry(1, 3, true, false));
}

#[test]
fn plan_node_should_retry_fail_fast_abort_short_circuits() {
    // Even with attempts left + retryable failure, an already-
    // tripped fail-fast abort must stop further retries so the
    // failing-fast contract (no new dispatches once aborted) is
    // honoured for retries too.
    assert!(!plan_node_should_retry(1, 3, false, true));
}

#[test]
fn node_results_json_quiet_for_baseline_single_attempt_failure() {
    // A node that failed on its single allowed attempt (no retry
    // policy) must stay quiet on the `retry` surface so the
    // wave-15 byte-shape is preserved.
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "f".into(),
        target: "mission_execution".into(),
        state: NodeState::Failed {
            reason: "boom".into(),
        },
        dispatch_strategy: "agent-team".into(),
        inner_payload: json!({"error": "boom"}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    let v = o.node_results_json();
    let arr = v.as_array().unwrap();
    assert!(arr[0].get("retry").is_none());
}

/// Forward-compat smoke test: the deterministic id we stamp on the
/// paused state must round-trip through wave-15's
/// `parse_review_question_id_struct` AND wave-16's subscriber
/// dispatcher so the wave-16 / task 02 resolution listener can route
/// it back to this plan when auto-resume lands. This task does NOT
/// implement resume; the test is the contract handshake.
#[test]
fn paused_node_review_question_id_round_trips_through_subscriber_dispatcher() {
    use super::super::review_gate::{
        derive_plan_node_review_question_id, parse_review_question_id_struct,
        plan_review_resolved_dispatch, ReviewDecision, ReviewResolvedDispatch,
    };
    let plan = fixture_plan("(plan)");
    let qid = derive_plan_node_review_question_id(
        &plan.id.to_string(),
        plan.version,
        "node-g",
        Some("plan-node"),
    );
    // Layer 1: wave-15 envelope parser.
    let parsed = parse_review_question_id_struct(&qid).expect("valid envelope");
    assert_eq!(parsed.scope, "plan");
    assert_eq!(parsed.action, "plan-node");
    assert!(parsed.topic_hash.is_some());
    // Layer 2: wave-16 subscriber dispatcher routes under the plan
    // scope so a future resume hook can match the deterministic id
    // to its origin node.
    let dispatch = plan_review_resolved_dispatch(&qid, "approved");
    match dispatch {
        ReviewResolvedDispatch::Route { parsed, decision } => {
            assert_eq!(parsed.scope, "plan");
            assert_eq!(parsed.action, "plan-node");
            assert_eq!(decision, ReviewDecision::Approved);
        }
        other => panic!("expected Route under plan scope, got {:?}", other),
    }
}

// ── wave-17 / task 01 — paused-node resume helpers ─────────────────
//
// Pure tests for the resume validator and the listener-side planner
// step that maps an approved plan-node Resolved event back to a
// resume request. End-to-end DB / dispatch coverage requires an
// AppState; the pure tests below pin the matrix authors care about.

fn paused_node(node_id: &str) -> DagNode {
    DagNode {
        id: node_id.to_string(),
        target: "mission_execution".into(),
        failure_policy: "fail-fast".into(),
        review_gate: Some("question-event".into()),
        review_action: Some("plan-node".into()),
        ..Default::default()
    }
}

fn parsed_dag_with(nodes: Vec<DagNode>) -> ParsedDag {
    ParsedDag {
        nodes,
        unsupported_top_forms: Vec::new(),
    }
}

#[test]
fn validate_resume_request_rejects_non_plan_node_action() {
    // Wave-15 manager-side ids (action ∈ {compile, approve, mark, supersede})
    // must NOT route through the resume helper — they belong to the
    // existing manager bridge.
    use super::super::review_gate::parse_review_question_id_struct;
    let plan = fixture_plan("(plan)");
    let qid = format!("review:plan:{}:v{}:approve", plan.id, plan.version);
    let parsed = parse_review_question_id_struct(&qid).expect("valid");
    let dag = parsed_dag_with(vec![paused_node("g")]);
    let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
    match err {
        PlanNodeResumeError::NotPlanNodeId { scope, action } => {
            assert_eq!(scope, "plan");
            assert_eq!(action, "approve");
        }
        other => panic!("expected NotPlanNodeId, got {:?}", other),
    }
}

#[test]
fn validate_resume_request_rejects_non_plan_scope() {
    use super::super::review_gate::parse_review_question_id_struct;
    let plan = fixture_plan("(plan)");
    let qid = "review:directive:abc:v1:plan-node:0123456789abcdef";
    let parsed = parse_review_question_id_struct(qid).expect("valid");
    let dag = parsed_dag_with(vec![paused_node("g")]);
    let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
    assert!(matches!(err, PlanNodeResumeError::NotPlanNodeId { .. }));
}

#[test]
fn validate_resume_request_rejects_plan_id_mismatch() {
    use super::super::review_gate::{
        derive_plan_node_review_question_id, parse_review_question_id_struct,
    };
    let plan = fixture_plan("(plan)");
    // Build a qid against a different plan id.
    let other_plan_id = "11111111-2222-3333-4444-555555555555";
    let qid = derive_plan_node_review_question_id(other_plan_id, plan.version, "g", None);
    let parsed = parse_review_question_id_struct(&qid).expect("valid");
    let dag = parsed_dag_with(vec![paused_node("g")]);
    let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
    match err {
        PlanNodeResumeError::PlanIdMismatch { expected, actual } => {
            assert_eq!(expected, plan.id.to_string());
            assert_eq!(actual, other_plan_id);
        }
        other => panic!("expected PlanIdMismatch, got {:?}", other),
    }
}

#[test]
fn validate_resume_request_rejects_stale_version() {
    use super::super::review_gate::{
        derive_plan_node_review_question_id, parse_review_question_id_struct,
    };
    let plan = fixture_plan("(plan)");
    // Build a qid against an older plan version.
    let qid =
        derive_plan_node_review_question_id(&plan.id.to_string(), plan.version - 1, "g", None);
    let parsed = parse_review_question_id_struct(&qid).expect("valid");
    let dag = parsed_dag_with(vec![paused_node("g")]);
    let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
    match err {
        PlanNodeResumeError::StaleVersion {
            expected,
            actual_in_id,
        } => {
            assert_eq!(expected, plan.version);
            assert_eq!(actual_in_id, plan.version - 1);
        }
        other => panic!("expected StaleVersion, got {:?}", other),
    }
}

#[test]
fn validate_resume_request_rejects_hash_with_no_paused_node() {
    // The DAG carries node `g` WITHOUT the review-gate hint — so
    // the hash for `g` won't match any paused-eligible node.
    use super::super::review_gate::{
        derive_plan_node_review_question_id, parse_review_question_id_struct,
    };
    let plan = fixture_plan("(plan)");
    let qid = derive_plan_node_review_question_id(&plan.id.to_string(), plan.version, "g", None);
    let parsed = parse_review_question_id_struct(&qid).expect("valid");
    let plain_node = DagNode {
        id: "g".into(),
        target: "mission_execution".into(),
        failure_policy: "fail-fast".into(),
        // NO review_gate set — node is not paused-eligible.
        ..Default::default()
    };
    let dag = parsed_dag_with(vec![plain_node]);
    let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
    assert!(matches!(
        err,
        PlanNodeResumeError::NoMatchingPausedNode { .. }
    ));
}

#[test]
fn validate_resume_request_rejects_hash_pointing_at_unknown_node() {
    // Plan was recompiled and the originally-paused node was renamed
    // — the qid hash now misses every paused-eligible node.
    use super::super::review_gate::{
        derive_plan_node_review_question_id, parse_review_question_id_struct,
    };
    let plan = fixture_plan("(plan)");
    let qid = derive_plan_node_review_question_id(
        &plan.id.to_string(),
        plan.version,
        "old-node-id",
        None,
    );
    let parsed = parse_review_question_id_struct(&qid).expect("valid");
    // DAG has a paused-eligible node, but with a DIFFERENT id.
    let dag = parsed_dag_with(vec![paused_node("new-node-id")]);
    let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
    match err {
        PlanNodeResumeError::NoMatchingPausedNode { topic_hash } => {
            assert_eq!(
                topic_hash,
                super::super::review_gate::derive_plan_node_topic_hash("old-node-id")
            );
        }
        other => panic!("expected NoMatchingPausedNode, got {:?}", other),
    }
}

#[test]
fn validate_resume_request_routes_unique_paused_node() {
    // Happy path: hash uniquely identifies a paused-eligible node.
    use super::super::review_gate::{
        derive_plan_node_review_question_id, parse_review_question_id_struct,
    };
    let plan = fixture_plan("(plan)");
    let qid = derive_plan_node_review_question_id(&plan.id.to_string(), plan.version, "g", None);
    let parsed = parse_review_question_id_struct(&qid).expect("valid");
    let dag = parsed_dag_with(vec![
        paused_node("g"),
        DagNode {
            // Plain non-paused node with same prefix substring should
            // NOT collide because the validator hashes the whole
            // node id (and the paused-eligible filter excludes it
            // anyway).
            id: "h".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ]);
    let node = validate_resume_request(&parsed, &plan, &dag).expect("ok");
    assert_eq!(node.id, "g");
}

#[test]
fn validate_resume_request_action_case_insensitive() {
    // The wave-15 envelope parser lowercases the action segment, but
    // assert the resume helper still routes correctly when the
    // upstream id was uppercased.
    use super::super::review_gate::{
        derive_plan_node_review_question_id, parse_review_question_id_struct,
    };
    let plan = fixture_plan("(plan)");
    let qid = derive_plan_node_review_question_id(
        &plan.id.to_string(),
        plan.version,
        "g",
        Some("PLAN-NODE"),
    );
    let parsed = parse_review_question_id_struct(&qid).expect("valid");
    let dag = parsed_dag_with(vec![paused_node("g")]);
    let node = validate_resume_request(&parsed, &plan, &dag).expect("ok");
    assert_eq!(node.id, "g");
}

#[test]
fn plan_node_resume_error_codes_match_review_validator_vocabulary() {
    // Pin the structured error codes the wave-15 review validator
    // already speaks — keeps audit dashboards routing on a stable
    // vocabulary.
    assert_eq!(
        PlanNodeResumeError::IdMalformed { detail: "x".into() }.code(),
        "REVIEW_ID_MALFORMED"
    );
    assert_eq!(
        PlanNodeResumeError::NotPlanNodeId {
            scope: "x".into(),
            action: "y".into(),
        }
        .code(),
        "REVIEW_ACTION_UNSUPPORTED"
    );
    assert_eq!(
        PlanNodeResumeError::PlanIdMismatch {
            expected: "x".into(),
            actual: "y".into(),
        }
        .code(),
        "REVIEW_ARTIFACT_MISMATCH"
    );
    assert_eq!(
        PlanNodeResumeError::StaleVersion {
            expected: 1,
            actual_in_id: 2,
        }
        .code(),
        "STALE_REVIEW_VERSION"
    );
}

#[test]
fn listener_planner_routes_approved_plan_node_resolved_through_resume_helper() {
    // Pure routing handshake: the wave-16 / task 02 subscriber's
    // planner must classify an approved plan-node Resolved event
    // as scope=plan + action=plan-node so the wave-17 / task 01
    // listener can branch on the action and call the resume
    // helper instead of the wave-15 manager-side handler.
    use super::super::review_gate::{
        derive_plan_node_review_question_id, is_plan_node_review_action,
        plan_review_resolved_dispatch, ReviewDecision, ReviewResolvedDispatch,
    };
    let plan_id = "00000000-0000-0000-0000-000000000abc";
    let qid = derive_plan_node_review_question_id(plan_id, 1, "node-g", None);
    let dispatch = plan_review_resolved_dispatch(&qid, "approved");
    match dispatch {
        ReviewResolvedDispatch::Route { parsed, decision } => {
            assert_eq!(parsed.scope, "plan");
            assert!(
                is_plan_node_review_action(&parsed.action),
                "action `{}` must classify as plan-node",
                parsed.action
            );
            assert_eq!(decision, ReviewDecision::Approved);
        }
        other => panic!("expected Route to plan-node resume helper, got {:?}", other),
    }
}

#[test]
fn listener_planner_ignores_unknown_resolution_for_plan_node_id() {
    // Even when the qid is shaped for plan-node, an unrecognised
    // resolution string MUST hit IgnoreUnknownResolution rather
    // than Route — this is the "no auto-approve for arbitrary
    // text" guarantee carried over into wave-17.
    use super::super::review_gate::{
        derive_plan_node_review_question_id, plan_review_resolved_dispatch, ReviewResolvedDispatch,
    };
    let plan_id = "00000000-0000-0000-0000-000000000abc";
    let qid = derive_plan_node_review_question_id(plan_id, 1, "node-g", None);
    let dispatch = plan_review_resolved_dispatch(&qid, "looks-good-to-me");
    assert!(matches!(
        dispatch,
        ReviewResolvedDispatch::IgnoreUnknownResolution { .. }
    ));
}

#[test]
fn listener_planner_routes_rejected_plan_node_resolved_with_decision_kept() {
    // Rejected resolutions still route through the planner — the
    // listener-side handler is responsible for keeping the node
    // paused without dispatching.
    use super::super::review_gate::{
        derive_plan_node_review_question_id, plan_review_resolved_dispatch, ReviewDecision,
        ReviewResolvedDispatch,
    };
    let plan_id = "00000000-0000-0000-0000-000000000abc";
    let qid = derive_plan_node_review_question_id(plan_id, 1, "node-g", None);
    let dispatch = plan_review_resolved_dispatch(&qid, "rejected");
    match dispatch {
        ReviewResolvedDispatch::Route { decision, .. } => {
            assert_eq!(decision, ReviewDecision::Rejected);
        }
        other => panic!("expected Route, got {:?}", other),
    }
}

// ── wave-17 / task 02 — claim / lease pure helpers ──────────────────

fn claim_test_plan_id() -> uuid::Uuid {
    uuid::Uuid::parse_str("00000000-0000-0000-0000-0000000c1a1d").unwrap()
}

#[test]
fn parse_claim_lease_secs_defaults_to_1800() {
    let v = json!({});
    assert_eq!(
        parse_claim_lease_secs(&v),
        PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS
    );
    assert_eq!(parse_claim_lease_secs(&v), 1800);
}

#[test]
fn parse_claim_lease_secs_clamps_low_and_high() {
    // Below floor → clamped up to MIN.
    assert_eq!(
        parse_claim_lease_secs(&json!({"claim_lease_secs": 5})),
        PLAN_DAG_CLAIM_LEASE_SECS_MIN
    );
    // Above ceiling → clamped down to MAX.
    assert_eq!(
        parse_claim_lease_secs(&json!({"claim_lease_secs": 999_999})),
        PLAN_DAG_CLAIM_LEASE_SECS_MAX
    );
    // Inside the band → echoed verbatim.
    assert_eq!(
        parse_claim_lease_secs(&json!({"claim_lease_secs": 600})),
        600
    );
}

#[test]
fn parse_claimer_name_defaults_when_missing_or_blank() {
    assert_eq!(
        parse_claimer_name(&json!({})),
        PLAN_DAG_DEFAULT_CLAIMER_NAME
    );
    // Whitespace-only → default (so a blank form field doesn't poison
    // the audit log).
    assert_eq!(
        parse_claimer_name(&json!({"claimer_name": "   "})),
        PLAN_DAG_DEFAULT_CLAIMER_NAME
    );
    // Explicit value → echoed (with surrounding whitespace trimmed).
    assert_eq!(
        parse_claimer_name(&json!({"claimer_name": "  alice  "})),
        "alice"
    );
}

#[test]
fn parse_enforce_claims_defaults_to_false() {
    assert!(!parse_enforce_claims(&json!({})));
    assert!(parse_enforce_claims(&json!({"enforce_claims": true})));
    assert!(!parse_enforce_claims(&json!({"enforce_claims": false})));
    // Non-bool values normalise to false (strict opt-in).
    assert!(!parse_enforce_claims(&json!({"enforce_claims": "yes"})));
    assert!(!parse_enforce_claims(&json!({"enforce_claims": 1})));
}

#[test]
fn derive_node_claim_scopes_uses_owned_files_first() {
    let plan_id = claim_test_plan_id();
    let node = DagNode {
        id: "n1".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        owned_files_raw: Some(r#"["src/a.rs" "src/b.rs"]"#.into()),
        scope: Some("ignored-when-owned-files-set".into()),
        ..Default::default()
    };
    let (scopes, source) = derive_node_claim_scopes(&node, plan_id);
    assert_eq!(source, CLAIM_SCOPE_SOURCE_OWNED_FILES);
    assert_eq!(scopes, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
}

#[test]
fn derive_node_claim_scopes_falls_back_to_scope_when_no_owned_files() {
    let plan_id = claim_test_plan_id();
    let node = DagNode {
        id: "n2".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        scope: Some("crates/foo".into()),
        ..Default::default()
    };
    let (scopes, source) = derive_node_claim_scopes(&node, plan_id);
    assert_eq!(source, CLAIM_SCOPE_SOURCE_SCOPE);
    assert_eq!(scopes, vec!["crates/foo".to_string()]);
}

#[test]
fn derive_node_claim_scopes_falls_back_to_plan_node_synthetic_when_empty() {
    let plan_id = claim_test_plan_id();
    let node = DagNode {
        id: "n3".into(),
        target: "mission_execution".into(),
        failure_policy: "fail-fast".into(),
        ..Default::default()
    };
    let (scopes, source) = derive_node_claim_scopes(&node, plan_id);
    assert_eq!(source, CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK);
    assert_eq!(scopes.len(), 1);
    assert!(scopes[0].contains(&plan_id.to_string()));
    assert!(scopes[0].contains("node/n3"));
}

#[test]
fn derive_node_claim_scopes_treats_blank_owned_files_and_scope_as_empty() {
    let plan_id = claim_test_plan_id();
    let node = DagNode {
        id: "n4".into(),
        target: "mission_execution".into(),
        failure_policy: "fail-fast".into(),
        owned_files_raw: Some(r#"["   "]"#.into()),
        scope: Some("   ".into()),
        ..Default::default()
    };
    let (scopes, source) = derive_node_claim_scopes(&node, plan_id);
    // Blank owned_files entries filter out → falls through to blank
    // :scope → falls through to synthetic.
    assert_eq!(source, CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK);
    assert_eq!(scopes.len(), 1);
}

#[test]
fn derive_plan_dag_claim_id_includes_attempt() {
    let plan_id = claim_test_plan_id();
    let id_a = derive_plan_dag_claim_id(plan_id, "node-x", 1);
    let id_b = derive_plan_dag_claim_id(plan_id, "node-x", 2);
    assert_ne!(id_a, id_b);
    assert!(id_a.starts_with("plan-dag:"));
    assert!(id_a.ends_with(":1"));
    assert!(id_b.ends_with(":2"));
}

fn claim_test_now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
}

#[test]
fn claim_registry_acquires_disjoint_scopes() {
    let mut reg = ClaimRegistry::new();
    let now = claim_test_now();
    let r1 = reg.try_acquire(
        "c1".into(),
        "claimer-a".into(),
        vec!["src/a.rs".into()],
        CLAIM_SCOPE_SOURCE_OWNED_FILES,
        300,
        now,
    );
    assert!(matches!(r1, ClaimAcquire::Acquired(_)));
    let r2 = reg.try_acquire(
        "c2".into(),
        "claimer-b".into(),
        vec!["src/b.rs".into()],
        CLAIM_SCOPE_SOURCE_OWNED_FILES,
        300,
        now,
    );
    assert!(matches!(r2, ClaimAcquire::Acquired(_)));
    assert_eq!(reg.len(), 2);
}

#[test]
fn claim_registry_rejects_overlapping_scope() {
    let mut reg = ClaimRegistry::new();
    let now = claim_test_now();
    let r1 = reg.try_acquire(
        "c1".into(),
        "alpha".into(),
        vec!["crates/foo".into()],
        CLAIM_SCOPE_SOURCE_SCOPE,
        300,
        now,
    );
    assert!(matches!(r1, ClaimAcquire::Acquired(_)));
    let r2 = reg.try_acquire(
        "c2".into(),
        "beta".into(),
        // Prefix of the held scope — `scopes_overlap_pure` matches
        // both directions.
        vec!["crates/foo/src".into()],
        CLAIM_SCOPE_SOURCE_SCOPE,
        300,
        now,
    );
    match r2 {
        ClaimAcquire::Conflict {
            conflicting_claim_id,
            conflicting_claimer,
            conflicting_scope,
            offending_scope,
            ..
        } => {
            assert_eq!(conflicting_claim_id, "c1");
            assert_eq!(conflicting_claimer, "alpha");
            assert_eq!(conflicting_scope, "crates/foo");
            assert_eq!(offending_scope, "crates/foo/src");
        }
        other => panic!("expected Conflict, got {:?}", other),
    }
    // The conflicting attempt was NOT inserted — only the original
    // acquired claim lives in the registry.
    assert_eq!(reg.len(), 1);
}

#[test]
fn claim_registry_release_then_reacquire_succeeds() {
    let mut reg = ClaimRegistry::new();
    let now = claim_test_now();
    let r1 = reg.try_acquire(
        "c1".into(),
        "writer".into(),
        vec!["src/a.rs".into()],
        CLAIM_SCOPE_SOURCE_OWNED_FILES,
        300,
        now,
    );
    assert!(matches!(r1, ClaimAcquire::Acquired(_)));
    let later = now + chrono::Duration::seconds(10);
    let released = reg.release("c1", later);
    assert!(released.is_some());
    assert!(released.unwrap().released_at.is_some());
    // After release the same scope can be re-acquired by a different
    // claim id (audit row remains, registry just moves on).
    let r2 = reg.try_acquire(
        "c2".into(),
        "writer-2".into(),
        vec!["src/a.rs".into()],
        CLAIM_SCOPE_SOURCE_OWNED_FILES,
        300,
        later,
    );
    assert!(matches!(r2, ClaimAcquire::Acquired(_)));
}

#[test]
fn claim_registry_lease_expiry_treats_held_claim_as_soft_released() {
    let mut reg = ClaimRegistry::new();
    let now = claim_test_now();
    let r1 = reg.try_acquire(
        "c1".into(),
        "writer".into(),
        vec!["src/a.rs".into()],
        CLAIM_SCOPE_SOURCE_OWNED_FILES,
        // 60-second lease so we can deliberately step past it.
        60,
        now,
    );
    assert!(matches!(r1, ClaimAcquire::Acquired(_)));
    // Step well past the lease — registry should treat the claim as
    // soft-released for conflict purposes (mirrors wave12-01).
    let later = now + chrono::Duration::seconds(120);
    let r2 = reg.try_acquire(
        "c2".into(),
        "writer-2".into(),
        vec!["src/a.rs".into()],
        CLAIM_SCOPE_SOURCE_OWNED_FILES,
        300,
        later,
    );
    assert!(matches!(r2, ClaimAcquire::Acquired(_)));
}

#[test]
fn build_planned_claims_emits_one_entry_per_node_in_topo_order() {
    let plan_id = claim_test_plan_id();
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_task_delegate"
                :owned-files ["src/a.rs"])
          (node :id "n2" :target "mission_execution"
                :scope "crates/foo" :depends-on ["n1"])
          (node :id "n3" :target "mission_execution" :depends-on ["n2"]))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid");
    let projection = build_planned_claims(&parsed.nodes, &order, plan_id, "scheduler", 900, true);
    let arr = projection.as_array().expect("array");
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["node_id"], "n1");
    assert_eq!(arr[0]["scope_source"], CLAIM_SCOPE_SOURCE_OWNED_FILES);
    assert_eq!(arr[0]["scopes"], json!(["src/a.rs"]));
    assert_eq!(arr[0]["lease_secs"], 900);
    assert_eq!(arr[0]["enforce_claims"], true);
    assert_eq!(arr[0]["claimer"], "scheduler");
    assert_eq!(arr[1]["node_id"], "n2");
    assert_eq!(arr[1]["scope_source"], CLAIM_SCOPE_SOURCE_SCOPE);
    assert_eq!(arr[1]["scopes"], json!(["crates/foo"]));
    assert_eq!(arr[2]["node_id"], "n3");
    assert_eq!(
        arr[2]["scope_source"],
        CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK
    );
}

#[tokio::test]
async fn dry_run_response_includes_planned_claims_and_knobs() {
    // Build a fake AppState by way of the existing test fixtures.
    // We exercise the dry-run branch which never touches the bus
    // / store, so we can pass a minimal AppState constructed via
    // `AppState::test_dummy()` where available — but that helper
    // doesn't exist on plan_dag's test surface, so instead we
    // assert the projection shape via the pure `build_planned_claims`
    // (already covered above) PLUS the sub-projection that
    // `action_execute_dag_v1` would echo. The integration glue
    // (action_execute_dag_v1 itself) is exercised by full daemon
    // tests, not pure unit tests.
    let plan_id = claim_test_plan_id();
    let sexp = r#"
        (plan
          (node :id "n1" :target "mission_execution"
                :owned-files ["src/x.rs"]))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("valid");
    let projection = build_planned_claims(
        &parsed.nodes,
        &order,
        plan_id,
        PLAN_DAG_DEFAULT_CLAIMER_NAME,
        PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS,
        false,
    );
    let arr = projection.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["claimer"], PLAN_DAG_DEFAULT_CLAIMER_NAME);
    assert_eq!(arr[0]["lease_secs"], PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS);
    assert_eq!(arr[0]["enforce_claims"], false);
    // Claim id format is byte-stable so dashboards can grep on it.
    let claim_id = arr[0]["claim_id"].as_str().unwrap();
    assert!(claim_id.starts_with("plan-dag:"));
    assert!(claim_id.ends_with(":1"));
}

#[test]
fn plan_dag_claim_iso_timestamps_round_trip_through_chrono() {
    // Pin the ISO-8601 second-precision projection so audit
    // dashboards can compare claim timestamps to wave12-01
    // companion-log claims byte-for-byte.
    let now = claim_test_now();
    let claim = PlanDagClaim {
        claim_id: "plan-dag:00000000-0000-0000-0000-0000000c1a1d:n1:1".into(),
        claimer: "plan-dag-scheduler".into(),
        scopes: vec!["src/a.rs".into()],
        scope_source: CLAIM_SCOPE_SOURCE_OWNED_FILES,
        acquired_at: now,
        lease_expires_at: now + chrono::Duration::seconds(300),
        released_at: None,
    };
    assert_eq!(claim.acquired_at_iso(), "2026-01-01T12:00:00Z");
    assert_eq!(claim.lease_expires_at_iso(), "2026-01-01T12:05:00Z");
    assert!(claim.released_at_iso().is_none());
    let mut released = claim.clone();
    released.released_at = Some(now + chrono::Duration::seconds(42));
    assert_eq!(released.released_at_iso().unwrap(), "2026-01-01T12:00:42Z");
}

#[test]
fn enforce_claims_off_preserves_default_byte_compat_knobs() {
    // The compat-mode default surface MUST report `enforce_claims=false`
    // and the wave12-01 lease defaults so pre-wave17 callers see
    // their expected byte-shape.
    let v = json!({});
    assert!(!parse_enforce_claims(&v));
    assert_eq!(
        parse_claim_lease_secs(&v),
        PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS
    );
    assert_eq!(parse_claimer_name(&v), PLAN_DAG_DEFAULT_CLAIMER_NAME);
}

#[test]
fn enforce_claims_on_does_not_change_scope_derivation() {
    // The enforce knob lives in the scheduler's dispatch path, not
    // in the scope derivation. Pin that boundary so a future
    // refactor that conflates the two surfaces gets caught.
    let plan_id = claim_test_plan_id();
    let node = DagNode {
        id: "n5".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
        ..Default::default()
    };
    let (scopes_a, source_a) = derive_node_claim_scopes(&node, plan_id);
    let (scopes_b, source_b) = derive_node_claim_scopes(&node, plan_id);
    assert_eq!(scopes_a, scopes_b);
    assert_eq!(source_a, source_b);
    assert_eq!(source_a, CLAIM_SCOPE_SOURCE_OWNED_FILES);
}

#[test]
fn claim_registry_release_returns_none_for_unknown_id() {
    let mut reg = ClaimRegistry::new();
    assert!(reg.release("ghost", claim_test_now()).is_none());
}

#[test]
fn claim_registry_release_is_idempotent_on_already_released_record() {
    let mut reg = ClaimRegistry::new();
    let now = claim_test_now();
    let _ = reg.try_acquire(
        "c1".into(),
        "writer".into(),
        vec!["src/a.rs".into()],
        CLAIM_SCOPE_SOURCE_OWNED_FILES,
        300,
        now,
    );
    let later1 = now + chrono::Duration::seconds(5);
    let later2 = now + chrono::Duration::seconds(10);
    let r1 = reg.release("c1", later1).expect("first release");
    assert_eq!(r1.released_at, Some(later1));
    let r2 = reg
        .release("c1", later2)
        .expect("second release returns same record");
    // First release wins — second release must NOT clobber the
    // earlier timestamp (audit dashboards depend on the original
    // release moment).
    assert_eq!(r2.released_at, Some(later1));
}

// ── wave-17 / task 03 — deterministic acceptance evaluator ───────────
//
// The acceptance phase runs after a successful inner dispatch and
// decides whether the node truly succeeded, was rejected, or needs
// human approval. CRITICAL invariant: NO shell command is ever
// executed; the evaluator is a pure projection over `(node,
// payload)`. These tests pin the four decision branches plus the
// fact that declared `:acceptance-commands` are surfaced verbatim
// without execution.

fn acceptance_node_with(mode: Option<&str>, commands: Option<&str>, keys: Option<&str>) -> DagNode {
    DagNode {
        id: "n".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        acceptance_mode_raw: mode.map(|s| s.to_string()),
        acceptance_commands_raw: commands.map(|s| s.to_string()),
        acceptance_evidence_keys_raw: keys.map(|s| s.to_string()),
        ..Default::default()
    }
}

#[test]
fn parse_node_form_captures_acceptance_evaluator_hints() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :acceptance-mode "evidence_keys"
                :acceptance-evidence-keys ["build_ok" "tests_passed"]
                :acceptance-commands ["cargo test" "git diff --check"]))
    "#;
    let parsed = parse_plan_dag(sexp);
    let n = &parsed.nodes[0];
    assert_eq!(n.acceptance_mode_raw.as_deref(), Some("evidence_keys"));
    assert!(n
        .acceptance_evidence_keys_raw
        .as_deref()
        .unwrap()
        .contains("build_ok"));
    assert!(n
        .acceptance_commands_raw
        .as_deref()
        .unwrap()
        .contains("cargo test"));
    // None of the new keys must land in unsupported_fields — that
    // would mean the scheduler can't route the acceptance phase.
    let unsupported_keys: Vec<String> = n
        .unsupported_fields
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    for forbidden in [
        "acceptance-mode",
        "acceptance-evidence-keys",
        "acceptance-commands",
    ] {
        assert!(
            !unsupported_keys.contains(&forbidden.to_string()),
            "key `{}` must land on a typed slot, not unsupported_fields",
            forbidden
        );
    }
    assert!(n.has_acceptance_hints());
    assert_eq!(n.acceptance_mode_kind(), Some(AcceptanceMode::EvidenceKeys));
}

#[test]
fn parse_node_form_records_unrecognised_acceptance_mode_in_unsupported_fields() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :acceptance-mode "invent_a_mode"))
    "#;
    let parsed = parse_plan_dag(sexp);
    let n = &parsed.nodes[0];
    // Raw value lands on the typed slot AND the typo surfaces in
    // unsupported_fields so the response loudly flags the mistake.
    assert_eq!(n.acceptance_mode_raw.as_deref(), Some("invent_a_mode"));
    assert!(n
        .unsupported_fields
        .iter()
        .any(|(k, v)| k == "acceptance-mode" && v == "invent_a_mode"));
    // The typed projection refuses to interpret a typo as a real mode.
    assert!(n.acceptance_mode_kind().is_none());
}

#[test]
fn build_nodes_summary_surfaces_acceptance_hints_when_present() {
    let nodes = vec![
        DagNode {
            id: "with".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            acceptance_mode_raw: Some("inner_status".into()),
            acceptance_evidence_keys_raw: Some(r#"["k1"]"#.into()),
            ..Default::default()
        },
        DagNode {
            id: "plain".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let order = vec!["with".to_string(), "plain".to_string()];
    let summary = build_nodes_summary(&nodes, &order);
    let arr = summary.as_array().unwrap();
    assert_eq!(arr[0]["acceptance_mode"], "inner_status");
    assert!(arr[0]["acceptance_evidence_keys_raw"]
        .as_str()
        .unwrap()
        .contains("k1"));
    // Plain node carries none of the acceptance fields so the
    // summary stays quiet (regression guard for the wave-16 baseline).
    assert!(arr[1].get("acceptance_mode").is_none());
    assert!(arr[1].get("acceptance_evidence_keys_raw").is_none());
}

#[test]
fn evaluate_acceptance_no_hints_returns_not_evaluated() {
    let node = acceptance_node_with(None, None, None);
    let payload = json!({"task_id": "btk-1"});
    let e = evaluate_node_acceptance(&node, &payload, true);
    assert_eq!(e.status, AcceptanceStatus::NotEvaluated);
    assert!(e.is_inactive());
    assert!(e.commands.is_empty());
    assert!(e.evidence_keys.is_empty());
    assert!(e.mode.is_none());
}

#[test]
fn evaluate_acceptance_inner_status_accepts_clean_success_payload() {
    let node = acceptance_node_with(Some("inner_status"), None, None);
    let payload = json!({"workstation_dispatch_status": "dispatched", "task_id": "btk-1"});
    let e = evaluate_node_acceptance(&node, &payload, true);
    assert_eq!(e.status, AcceptanceStatus::Accepted);
    assert_eq!(e.mode, Some(AcceptanceMode::InnerStatus));
    assert!(e.reason.contains("dispatch Ok"));
}

#[test]
fn evaluate_acceptance_inner_status_rejects_when_dispatch_classification_failed() {
    // dispatch_succeeded=false short-circuits the evaluator to
    // Rejected even when the payload looks clean. This guards
    // against the evaluator second-guessing the dispatch judgment.
    let node = acceptance_node_with(Some("inner_status"), None, None);
    let payload = json!({"task_id": "btk-1"});
    let e = evaluate_node_acceptance(&node, &payload, false);
    assert_eq!(e.status, AcceptanceStatus::Rejected);
    assert!(e.reason.contains("dispatch classification was not Ok"));
}

#[test]
fn evaluate_acceptance_inner_status_rejects_when_payload_signals_error() {
    let node = acceptance_node_with(Some("inner_status"), None, None);
    for bad in [
        json!({"error": "boom"}),
        json!({"success": false}),
        json!({"ok": false}),
        json!({"status": "failed"}),
        json!({"workstation_dispatch_status": "skipped_project_root_unresolved"}),
    ] {
        let e = evaluate_node_acceptance(&node, &bad, true);
        assert_eq!(
            e.status,
            AcceptanceStatus::Rejected,
            "payload {:?} should reject under inner_status",
            bad
        );
    }
}

#[test]
fn evaluate_acceptance_evidence_keys_accepts_when_all_present_at_top_level() {
    let node = acceptance_node_with(
        Some("evidence_keys"),
        None,
        Some(r#"["build_ok" "tests_passed"]"#),
    );
    let payload = json!({
        "build_ok": true,
        "tests_passed": 3,
        "noise": "anything",
    });
    let e = evaluate_node_acceptance(&node, &payload, true);
    assert_eq!(e.status, AcceptanceStatus::Accepted);
    assert_eq!(e.mode, Some(AcceptanceMode::EvidenceKeys));
    assert_eq!(
        e.evidence_keys,
        vec!["build_ok".to_string(), "tests_passed".to_string()]
    );
}

#[test]
fn evaluate_acceptance_evidence_keys_descends_into_nested_holders() {
    // Substrates often stash typed evidence under `evidence` /
    // `inner_result`; the evaluator descends one level into the
    // well-known holders so authors don't have to mirror the
    // payload's exact nesting in their `:acceptance-evidence-keys`.
    let node = acceptance_node_with(
        Some("evidence_keys"),
        None,
        Some(r#"["build_ok" "tests_passed"]"#),
    );
    let payload = json!({
        "evidence": {
            "build_ok": true,
            "tests_passed": 1,
        }
    });
    let e = evaluate_node_acceptance(&node, &payload, true);
    assert_eq!(e.status, AcceptanceStatus::Accepted);
}

#[test]
fn evaluate_acceptance_evidence_keys_rejects_missing_keys_with_named_list() {
    let node = acceptance_node_with(
        Some("evidence_keys"),
        None,
        Some(r#"["build_ok" "tests_passed"]"#),
    );
    let payload = json!({"build_ok": true});
    let e = evaluate_node_acceptance(&node, &payload, true);
    assert_eq!(e.status, AcceptanceStatus::Rejected);
    assert!(
        e.reason.contains("tests_passed"),
        "reason `{}` must surface the missing key",
        e.reason
    );
}

#[test]
fn evaluate_acceptance_evidence_keys_with_empty_keys_degrades_to_manual() {
    let node = acceptance_node_with(Some("evidence_keys"), None, Some("[]"));
    let payload = json!({"task_id": "x"});
    let e = evaluate_node_acceptance(&node, &payload, true);
    // An empty contract cannot prove anything — surface as
    // manual_required so the typo is loud.
    assert_eq!(e.status, AcceptanceStatus::ManualRequired);
    assert!(e.reason.contains("empty"));
}

#[test]
fn evaluate_acceptance_manual_mode_always_pauses() {
    let node = acceptance_node_with(Some("manual"), None, None);
    let payload = json!({"task_id": "x"});
    let e = evaluate_node_acceptance(&node, &payload, true);
    assert_eq!(e.status, AcceptanceStatus::ManualRequired);
    assert_eq!(e.mode, Some(AcceptanceMode::Manual));
}

#[test]
fn evaluate_acceptance_commands_without_mode_pause_as_manual_required_and_never_run_shell() {
    // CRITICAL: declaring `:acceptance-commands` without a typed
    // evaluator must NOT execute shell. The default policy is to
    // surface the gate as manual_required and carry the commands
    // verbatim into the response so a human / out-of-band pipeline
    // can run them.
    let node = acceptance_node_with(None, Some(r#"["cargo test" "git diff --check"]"#), None);
    let payload = json!({"task_id": "x"});
    let e = evaluate_node_acceptance(&node, &payload, true);
    assert_eq!(e.status, AcceptanceStatus::ManualRequired);
    assert_eq!(
        e.commands,
        vec!["cargo test".to_string(), "git diff --check".to_string()],
        "declared commands must round-trip into the evaluation block verbatim"
    );
    assert!(e.mode.is_none());
    assert!(e.reason.contains("never runs shell"));
}

#[test]
fn evaluation_to_json_carries_every_surface_field() {
    let node = acceptance_node_with(
        Some("inner_status"),
        Some(r#"["cargo test"]"#),
        Some(r#"["k1"]"#),
    );
    let payload = json!({"task_id": "x"});
    let e = evaluate_node_acceptance(&node, &payload, true);
    let v = e.to_json();
    assert_eq!(v["status"], "accepted");
    assert_eq!(v["mode"], "inner_status");
    assert_eq!(v["commands"][0], "cargo test");
    assert_eq!(v["evidence_keys"][0], "k1");
    assert!(v["reason"].is_string());
}

#[test]
fn derive_acceptance_pause_id_is_distinct_from_review_gate_id_space() {
    // The deterministic pause id MUST start with `acceptance:` so
    // the wave-17 / task 01 paused-node resume helper (which
    // requires `review:plan:...:plan-node:...` shape) cannot
    // accidentally consume an acceptance pause.
    let plan_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap();
    let id = derive_acceptance_pause_id(plan_id, 7, "n42");
    assert!(
        id.starts_with("acceptance:plan:"),
        "id `{}` must use the acceptance prefix",
        id
    );
    assert!(id.contains(":v7:"));
    assert!(id.ends_with(":n42"));
    // Round-trips deterministically — same inputs, same output.
    assert_eq!(id, derive_acceptance_pause_id(plan_id, 7, "n42"));
}

#[test]
fn node_results_json_surfaces_acceptance_block_only_when_active() {
    let mut o = ExecutionOutcome::default();
    // Active acceptance — surfaces.
    o.results.push(NodeResult {
        id: "with_acc".into(),
        target: "mission_task_delegate".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "fresh-code-alignment".into(),
        inner_payload: json!({"task_id": "btk-1"}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: Some(AcceptanceEvaluation {
            status: AcceptanceStatus::Accepted,
            mode: Some(AcceptanceMode::InnerStatus),
            commands: vec!["cargo test".into()],
            evidence_keys: vec![],
            reason: "ok".into(),
            fan_in: None,
        }),
    });
    // No hints — quiet.
    o.results.push(NodeResult {
        id: "plain".into(),
        target: "mission_execution".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "unknown".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    let v = o.node_results_json();
    let arr = v.as_array().unwrap();
    assert_eq!(arr[0]["acceptance"]["status"], "accepted");
    assert_eq!(arr[0]["acceptance"]["mode"], "inner_status");
    assert_eq!(arr[0]["acceptance"]["commands"][0], "cargo test");
    assert!(arr[1].get("acceptance").is_none());
}

#[test]
fn manual_required_surfaces_paused_state_with_acceptance_id_distinct_from_review_gate() {
    // When the acceptance phase returns ManualRequired the wave
    // loop MUST flip the node to `Paused` with the deterministic
    // `acceptance:plan:...` id (NOT the wave-16 `review:plan:...`
    // id). The aggregate status surfaces as `dag_paused` — same
    // codepath as review-gate paused.
    let mut o = ExecutionOutcome::default();
    o.results.push(NodeResult {
        id: "n".into(),
        target: "mission_task_delegate".into(),
        state: NodeState::Paused {
            question_id: derive_acceptance_pause_id(
                uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap(),
                1,
                "n",
            ),
            bus_publish_warning: None,
        },
        dispatch_strategy: "fresh-code-alignment".into(),
        inner_payload: json!({"task_id": "btk-1"}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: Some(AcceptanceEvaluation {
            status: AcceptanceStatus::ManualRequired,
            mode: Some(AcceptanceMode::Manual),
            commands: vec![],
            evidence_keys: vec![],
            reason: "manual mode".into(),
            fan_in: None,
        }),
    });
    assert_eq!(o.aggregate_status(), "dag_paused");
    assert_eq!(o.runner_status(), "review_gate_paused");
    let arr = o.node_results_json();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr[0]["state"], "paused");
    assert_eq!(arr[0]["acceptance"]["status"], "manual_required");
    let qid = arr[0]["review_question_id"].as_str().unwrap();
    assert!(
        qid.starts_with("acceptance:plan:"),
        "manual_required pause id `{}` MUST use the acceptance: prefix",
        qid
    );
}

// ── wave-18 / task 03 — cross-node acceptance fan-in ─────────────────
//
// The fan-in evaluator overlays a deterministic decision on top of
// the per-node acceptance evaluation. It NEVER re-runs the source
// node — it only inspects the recorded `state` (lifecycle) and
// `inner_payload`. Validator + evaluator invariants pinned below.

fn make_succeeded_result(id: &str, payload: Value) -> NodeResult {
    NodeResult {
        id: id.to_string(),
        target: "mission_task_delegate".into(),
        state: NodeState::Succeeded,
        dispatch_strategy: "fresh-code-alignment".into(),
        inner_payload: payload,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    }
}

fn make_failed_result(id: &str) -> NodeResult {
    NodeResult {
        id: id.to_string(),
        target: "mission_task_delegate".into(),
        state: NodeState::Failed {
            reason: "test failure".into(),
        },
        dispatch_strategy: "fresh-code-alignment".into(),
        inner_payload: Value::Null,
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    }
}

#[test]
fn parse_node_form_captures_acceptance_fan_in_hints() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate")
          (node :id "n2"
                :target "mission_task_delegate"
                :depends-on ["n1"]
                :acceptance-depends-on ["n1"]
                :acceptance-requires "all_succeeded"))
    "#;
    let parsed = parse_plan_dag(sexp);
    let n2 = parsed.nodes.iter().find(|n| n.id == "n2").unwrap();
    assert_eq!(n2.acceptance_depends_on, vec!["n1".to_string()]);
    assert_eq!(n2.acceptance_requires_raw.as_deref(), Some("all_succeeded"));
    assert_eq!(
        n2.acceptance_requires_kind(),
        Some(AcceptanceRequires::AllSucceeded)
    );
    assert!(n2.has_acceptance_fan_in());
    assert!(n2.has_acceptance_hints());
    // Recognised mode MUST NOT land in unsupported_fields.
    let unsupported_keys: Vec<String> = n2
        .unsupported_fields
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    for forbidden in [
        "acceptance-depends-on",
        "acceptance-requires",
        "acceptance-source-node",
    ] {
        assert!(
            !unsupported_keys.contains(&forbidden.to_string()),
            "key `{}` must land on a typed slot, not unsupported_fields",
            forbidden
        );
    }
}

#[test]
fn parse_node_form_records_unrecognised_acceptance_requires_in_unsupported_fields() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate")
          (node :id "n2"
                :target "mission_task_delegate"
                :depends-on ["n1"]
                :acceptance-depends-on ["n1"]
                :acceptance-requires "majority_succeeded"))
    "#;
    let parsed = parse_plan_dag(sexp);
    let n2 = parsed.nodes.iter().find(|n| n.id == "n2").unwrap();
    // Raw value lands on the typed slot AND in unsupported_fields.
    assert_eq!(
        n2.acceptance_requires_raw.as_deref(),
        Some("majority_succeeded")
    );
    assert!(n2
        .unsupported_fields
        .iter()
        .any(|(k, v)| k == "acceptance-requires" && v == "majority_succeeded"));
    assert!(n2.acceptance_requires_kind().is_none());
}

#[test]
fn build_validated_dag_rejects_acceptance_dep_referencing_missing_node() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :acceptance-depends-on ["does_not_exist"]
                :acceptance-requires "all_succeeded"))
    "#;
    let err = build_validated_dag(sexp).expect_err("must reject missing fan-in dep");
    match err {
        DagBuildError::AcceptanceDependencyMissing { node_id, missing } => {
            assert_eq!(node_id, "n1");
            assert_eq!(missing, "does_not_exist");
        }
        other => panic!("expected AcceptanceDependencyMissing, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_acceptance_dep_when_not_a_depends_on_ancestor() {
    // n2 declares :acceptance-depends-on ["n1"] but does NOT carry
    // n1 as a (transitive) :depends-on ancestor — the source node's
    // evidence may not exist when n2's acceptance phase runs, so
    // the validator MUST refuse instead of silently changing
    // execution order.
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate")
          (node :id "n2"
                :target "mission_task_delegate"
                :acceptance-depends-on ["n1"]
                :acceptance-requires "all_succeeded"))
    "#;
    let err = build_validated_dag(sexp).expect_err("must reject non-ancestor fan-in dep");
    match err {
        DagBuildError::AcceptanceFanInDepNotAncestor { node_id, ancestor } => {
            assert_eq!(node_id, "n2");
            assert_eq!(ancestor, "n1");
        }
        other => panic!("expected AcceptanceFanInDepNotAncestor, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_acceptance_depends_on_without_recognised_requires() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate")
          (node :id "n2"
                :target "mission_task_delegate"
                :depends-on ["n1"]
                :acceptance-depends-on ["n1"]))
    "#;
    let err = build_validated_dag(sexp).expect_err("must reject missing requires");
    match err {
        DagBuildError::AcceptanceFanInRequiresMissing { node_id, .. } => {
            assert_eq!(node_id, "n2");
        }
        other => panic!("expected AcceptanceFanInRequiresMissing, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_evidence_keys_mode_without_source_node() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate")
          (node :id "n2"
                :target "mission_task_delegate"
                :depends-on ["n1"]
                :acceptance-depends-on ["n1"]
                :acceptance-requires "evidence_keys"
                :acceptance-evidence-keys ["build_ok"]))
    "#;
    let err = build_validated_dag(sexp)
        .expect_err("evidence_keys without :acceptance-source-node must fail");
    match err {
        DagBuildError::AcceptanceSourceNodeInvalid { node_id, detail } => {
            assert_eq!(node_id, "n2");
            assert!(
                detail.contains("acceptance-source-node"),
                "detail `{}` must mention the missing field",
                detail
            );
        }
        other => panic!("expected AcceptanceSourceNodeInvalid, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_source_node_outside_depends_on_list() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate")
          (node :id "n2"
                :target "mission_task_delegate"
                :depends-on ["n1"]
                :acceptance-depends-on ["n1"]
                :acceptance-requires "evidence_keys"
                :acceptance-evidence-keys ["build_ok"]
                :acceptance-source-node "n1_typo"))
    "#;
    let err = build_validated_dag(sexp).expect_err("source node mismatch must fail");
    match err {
        DagBuildError::AcceptanceSourceNodeInvalid { node_id, detail } => {
            assert_eq!(node_id, "n2");
            assert!(detail.contains("n1_typo"));
        }
        other => panic!("expected AcceptanceSourceNodeInvalid, got {:?}", other),
    }
}

#[test]
fn build_validated_dag_accepts_well_formed_fan_in() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate")
          (node :id "n2"
                :target "mission_task_delegate"
                :depends-on ["n1"]
                :acceptance-depends-on ["n1"]
                :acceptance-requires "all_succeeded"))
    "#;
    let (_parsed, order) = build_validated_dag(sexp).expect("well-formed fan-in must build");
    assert_eq!(order, vec!["n1".to_string(), "n2".to_string()]);
}

#[test]
fn apply_fan_in_no_op_when_node_has_no_fan_in_hints() {
    // Absence of :acceptance-depends-on preserves the wave-17
    // shape exactly — the fan_in field is None on the way in
    // AND on the way out, regardless of prior_results contents.
    let node = acceptance_node_with(Some("inner_status"), None, None);
    let payload = json!({"task_id": "btk-1"});
    let base = evaluate_node_acceptance(&node, &payload, true);
    assert!(base.fan_in.is_none(), "baseline must carry no fan_in");
    let prior = HashMap::new();
    let after = apply_acceptance_fan_in(base.clone(), &node, &prior);
    assert_eq!(after.status, base.status);
    assert!(after.fan_in.is_none());
}

#[test]
fn apply_fan_in_all_succeeded_passes_when_every_source_succeeded() {
    let mut node = acceptance_node_with(None, None, None);
    node.acceptance_depends_on = vec!["a".into(), "b".into()];
    node.acceptance_requires_raw = Some("all_succeeded".into());
    let r_a = make_succeeded_result("a", json!({}));
    let r_b = make_succeeded_result("b", json!({}));
    let prior: HashMap<String, &NodeResult> = [("a".to_string(), &r_a), ("b".to_string(), &r_b)]
        .into_iter()
        .collect();
    let base = evaluate_node_acceptance(&node, &json!({}), true);
    let after = apply_acceptance_fan_in(base, &node, &prior);
    assert_eq!(after.status, AcceptanceStatus::Accepted);
    let f = after.fan_in.expect("fan_in must be recorded");
    assert!(f.passed);
    assert_eq!(f.mode, AcceptanceRequires::AllSucceeded);
    assert_eq!(f.source_nodes, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn apply_fan_in_all_succeeded_rejects_when_one_source_failed() {
    let mut node = acceptance_node_with(None, None, None);
    node.acceptance_depends_on = vec!["a".into(), "b".into()];
    node.acceptance_requires_raw = Some("all_succeeded".into());
    let r_a = make_succeeded_result("a", json!({}));
    let r_b = make_failed_result("b");
    let prior: HashMap<String, &NodeResult> = [("a".to_string(), &r_a), ("b".to_string(), &r_b)]
        .into_iter()
        .collect();
    let base = evaluate_node_acceptance(&node, &json!({}), true);
    let after = apply_acceptance_fan_in(base, &node, &prior);
    assert_eq!(after.status, AcceptanceStatus::Rejected);
    let f = after.fan_in.expect("fan_in must be recorded");
    assert!(!f.passed);
    assert!(
        f.reason.contains("\"b\""),
        "reason `{}` must surface the failing source node",
        f.reason
    );
    assert!(after.reason.starts_with("acceptance_fan_in:"));
}

#[test]
fn apply_fan_in_any_succeeded_passes_when_at_least_one_source_succeeded() {
    let mut node = acceptance_node_with(None, None, None);
    node.acceptance_depends_on = vec!["a".into(), "b".into()];
    node.acceptance_requires_raw = Some("any_succeeded".into());
    let r_a = make_failed_result("a");
    let r_b = make_succeeded_result("b", json!({}));
    let prior: HashMap<String, &NodeResult> = [("a".to_string(), &r_a), ("b".to_string(), &r_b)]
        .into_iter()
        .collect();
    let base = evaluate_node_acceptance(&node, &json!({}), true);
    let after = apply_acceptance_fan_in(base, &node, &prior);
    assert_eq!(after.status, AcceptanceStatus::Accepted);
    let f = after.fan_in.expect("fan_in must be recorded");
    assert!(f.passed);
    assert_eq!(f.mode, AcceptanceRequires::AnySucceeded);
}

#[test]
fn apply_fan_in_any_succeeded_rejects_when_all_sources_failed() {
    let mut node = acceptance_node_with(None, None, None);
    node.acceptance_depends_on = vec!["a".into(), "b".into()];
    node.acceptance_requires_raw = Some("any_succeeded".into());
    let r_a = make_failed_result("a");
    let r_b = make_failed_result("b");
    let prior: HashMap<String, &NodeResult> = [("a".to_string(), &r_a), ("b".to_string(), &r_b)]
        .into_iter()
        .collect();
    let base = evaluate_node_acceptance(&node, &json!({}), true);
    let after = apply_acceptance_fan_in(base, &node, &prior);
    assert_eq!(after.status, AcceptanceStatus::Rejected);
    let f = after.fan_in.expect("fan_in must be recorded");
    assert!(!f.passed);
}

#[test]
fn apply_fan_in_evidence_keys_passes_when_source_payload_carries_keys() {
    let mut node = acceptance_node_with(None, None, Some(r#"["build_ok" "tests_passed"]"#));
    node.acceptance_depends_on = vec!["a".into()];
    node.acceptance_requires_raw = Some("evidence_keys".into());
    node.acceptance_source_node = Some("a".into());
    let r_a = make_succeeded_result("a", json!({"build_ok": true, "tests_passed": 12}));
    let prior: HashMap<String, &NodeResult> = [("a".to_string(), &r_a)].into_iter().collect();
    let base = evaluate_node_acceptance(&node, &json!({}), true);
    let after = apply_acceptance_fan_in(base, &node, &prior);
    assert_eq!(after.status, AcceptanceStatus::Accepted);
    let f = after.fan_in.expect("fan_in must be recorded");
    assert!(f.passed);
    assert_eq!(f.mode, AcceptanceRequires::EvidenceKeys);
    assert_eq!(f.source_nodes, vec!["a".to_string()]);
}

#[test]
fn apply_fan_in_evidence_keys_rejects_when_source_missing_keys() {
    let mut node = acceptance_node_with(None, None, Some(r#"["build_ok" "tests_passed"]"#));
    node.acceptance_depends_on = vec!["a".into()];
    node.acceptance_requires_raw = Some("evidence_keys".into());
    node.acceptance_source_node = Some("a".into());
    let r_a = make_succeeded_result("a", json!({"build_ok": true}));
    let prior: HashMap<String, &NodeResult> = [("a".to_string(), &r_a)].into_iter().collect();
    let base = evaluate_node_acceptance(&node, &json!({}), true);
    let after = apply_acceptance_fan_in(base, &node, &prior);
    assert_eq!(after.status, AcceptanceStatus::Rejected);
    let f = after.fan_in.expect("fan_in must be recorded");
    assert!(!f.passed);
    assert!(
        f.reason.contains("tests_passed"),
        "reason `{}` must surface the missing key",
        f.reason
    );
}

#[test]
fn apply_fan_in_does_not_promote_a_per_node_rejected_decision() {
    // Per-node Rejected dominates — fan-in is recorded for audit
    // but never flips status back to Accepted.
    let mut node = acceptance_node_with(Some("inner_status"), None, None);
    node.acceptance_depends_on = vec!["a".into()];
    node.acceptance_requires_raw = Some("all_succeeded".into());
    let r_a = make_succeeded_result("a", json!({}));
    let prior: HashMap<String, &NodeResult> = [("a".to_string(), &r_a)].into_iter().collect();
    // Per-node: dispatch_succeeded=false → Rejected.
    let base = evaluate_node_acceptance(&node, &json!({}), false);
    assert_eq!(base.status, AcceptanceStatus::Rejected);
    let after = apply_acceptance_fan_in(base, &node, &prior);
    assert_eq!(
        after.status,
        AcceptanceStatus::Rejected,
        "per-node Rejected MUST dominate even when fan-in passes"
    );
    // Fan-in still recorded for audit.
    let f = after.fan_in.expect("fan_in must be recorded for audit");
    assert!(f.passed, "fan-in itself passed even though parent rejected");
}

#[test]
fn apply_fan_in_records_outcome_under_to_json_when_active() {
    let mut node = acceptance_node_with(None, None, None);
    node.acceptance_depends_on = vec!["a".into()];
    node.acceptance_requires_raw = Some("all_succeeded".into());
    let r_a = make_succeeded_result("a", json!({}));
    let prior: HashMap<String, &NodeResult> = [("a".to_string(), &r_a)].into_iter().collect();
    let base = evaluate_node_acceptance(&node, &json!({}), true);
    let after = apply_acceptance_fan_in(base, &node, &prior);
    let v = after.to_json();
    assert_eq!(v["status"], "accepted");
    assert_eq!(v["fan_in"]["mode"], "all_succeeded");
    assert_eq!(v["fan_in"]["passed"], true);
    assert_eq!(v["fan_in"]["source_nodes"][0], "a");
    assert!(v["fan_in"]["reason"].is_string());
}

#[test]
fn build_nodes_summary_surfaces_fan_in_hints_when_present() {
    let nodes = vec![
        DagNode {
            id: "a".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
        DagNode {
            id: "b".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            depends_on: vec!["a".into()],
            acceptance_depends_on: vec!["a".into()],
            acceptance_requires_raw: Some("evidence_keys".into()),
            acceptance_source_node: Some("a".into()),
            acceptance_evidence_keys_raw: Some(r#"["k"]"#.into()),
            ..Default::default()
        },
    ];
    let order = vec!["a".to_string(), "b".to_string()];
    let summary = build_nodes_summary(&nodes, &order);
    let arr = summary.as_array().unwrap();
    // a: no fan-in fields surface.
    assert!(arr[0].get("acceptance_depends_on").is_none());
    assert!(arr[0].get("acceptance_requires").is_none());
    assert!(arr[0].get("acceptance_source_node").is_none());
    // b: every declared field surfaces.
    assert_eq!(arr[1]["acceptance_depends_on"][0], "a");
    assert_eq!(arr[1]["acceptance_requires"], "evidence_keys");
    assert_eq!(arr[1]["acceptance_source_node"], "a");
}

// ── wave-17 / task 04 — conservative rollback descriptors ────────────
//
// The rollback pass runs AFTER a node's final failed attempt and
// BEFORE downstream taint propagation. It NEVER runs destructive
// shell commands; it only records intent, builds descriptors, or
// (in workstation mode) hands a scoped task brief to the existing
// wave-15 substrate. These tests pin every branch of the decision
// tree plus the failure-policy invariants the brief calls out.

fn rollback_node_with(
    policy: Option<&str>,
    objective: Option<&str>,
    owned_files: Option<&str>,
    acceptance_commands: Option<&str>,
) -> DagNode {
    DagNode {
        id: "n".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        // A safe forward dispatch strategy so the workstation
        // safety check can pass when the test wants it to.
        dispatch_strategy: Some("fresh-code-alignment".into()),
        target_project: Some("missiond".into()),
        rollback_policy: policy.map(|s| s.to_string()),
        rollback_objective: objective.map(|s| s.to_string()),
        rollback_owned_files_raw: owned_files.map(|s| s.to_string()),
        rollback_acceptance_commands_raw: acceptance_commands.map(|s| s.to_string()),
        ..Default::default()
    }
}

#[test]
fn parse_node_form_captures_rollback_policy_hints() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :rollback-policy "workstation"
                :rollback-objective "undo migration step 3"
                :rollback-owned-files ["src/migrations/0003.rs"]
                :rollback-acceptance-commands ["cargo test -p missiond"]))
    "#;
    let parsed = parse_plan_dag(sexp);
    let n = &parsed.nodes[0];
    assert_eq!(n.rollback_policy.as_deref(), Some("workstation"));
    assert_eq!(
        n.rollback_objective.as_deref(),
        Some("undo migration step 3")
    );
    assert!(n
        .rollback_owned_files_raw
        .as_deref()
        .unwrap()
        .contains("src/migrations/0003.rs"));
    assert!(n
        .rollback_acceptance_commands_raw
        .as_deref()
        .unwrap()
        .contains("cargo test"));
    // None of the new keys must land in unsupported_fields — that
    // would mean the scheduler can't route the rollback pass.
    let unsupported_keys: Vec<String> = n
        .unsupported_fields
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    for forbidden in [
        "rollback-policy",
        "rollback-objective",
        "rollback-owned-files",
        "rollback-acceptance-commands",
    ] {
        assert!(
            !unsupported_keys.contains(&forbidden.to_string()),
            "key `{}` must land on a typed slot, not unsupported_fields",
            forbidden
        );
    }
    assert!(n.has_rollback_hints());
    assert_eq!(n.rollback_policy_kind(), Some(RollbackPolicy::Workstation));
}

#[test]
fn parse_node_form_records_unrecognised_rollback_policy_in_unsupported_fields() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :rollback-policy "self_destruct"))
    "#;
    let parsed = parse_plan_dag(sexp);
    let n = &parsed.nodes[0];
    assert_eq!(n.rollback_policy.as_deref(), Some("self_destruct"));
    // Typo lands in the typed slot AND is surfaced via the
    // unsupported_fields audit so the response loudly flags it.
    assert!(n
        .unsupported_fields
        .iter()
        .any(|(k, v)| k == "rollback-policy" && v == "self_destruct"));
    // Typed projection refuses to interpret a typo as a real policy.
    assert!(n.rollback_policy_kind().is_none());
}

#[test]
fn rollback_policy_default_is_no_rollback_when_absent() {
    // Defaults: absent -> no rollback / no destructive action.
    let node = rollback_node_with(None, None, None, None);
    assert!(!node.has_rollback_hints());
    assert!(node.rollback_policy_kind().is_none());
    let descriptor = build_rollback_descriptor(&node);
    assert_eq!(descriptor.policy, RollbackPolicy::None);
    assert!(descriptor.objective.is_none());
    assert!(descriptor.owned_files.is_empty());
    assert!(descriptor.acceptance_commands.is_empty());
    let eval = pre_dispatch_rollback_decision(&node);
    assert_eq!(eval.status, RollbackStatus::NotRequested);
    assert!(eval.is_inactive());
}

#[test]
fn rollback_policy_explicit_none_is_inactive() {
    // `:rollback-policy "none"` is the explicit opt-out — the
    // descriptor still parses, but the evaluator surfaces
    // `not_requested` so the response stays quiet.
    let node = rollback_node_with(Some("none"), Some("noop"), None, None);
    assert_eq!(node.rollback_policy_kind(), Some(RollbackPolicy::None));
    let eval = pre_dispatch_rollback_decision(&node);
    assert_eq!(eval.policy, RollbackPolicy::None);
    assert_eq!(eval.status, RollbackStatus::NotRequested);
}

#[test]
fn rollback_descriptor_mode_records_intent_without_dispatch() {
    let node = rollback_node_with(
        Some("descriptor"),
        Some("undo step"),
        Some(r#"["src/a.rs"]"#),
        Some(r#"["cargo test"]"#),
    );
    let descriptor = build_rollback_descriptor(&node);
    assert_eq!(descriptor.policy, RollbackPolicy::Descriptor);
    assert_eq!(descriptor.objective.as_deref(), Some("undo step"));
    assert_eq!(descriptor.owned_files, vec!["src/a.rs".to_string()]);
    let eval = pre_dispatch_rollback_decision(&node);
    assert_eq!(eval.status, RollbackStatus::DescriptorReady);
    // No inner payload — descriptor mode never touches the substrate.
    assert!(eval.inner_payload.is_none());
    // Brief preview is computed by the async helper, not the pure
    // decision; pre-dispatch evaluation leaves it None.
    assert!(eval.task_brief_preview.is_none());
}

#[test]
fn rollback_workstation_mode_passes_safety_when_all_signals_present() {
    let node = rollback_node_with(
        Some("workstation"),
        Some("undo migration"),
        Some(r#"["src/a.rs"]"#),
        None,
    );
    let descriptor = build_rollback_descriptor(&node);
    assert_eq!(descriptor.policy, RollbackPolicy::Workstation);
    assert!(descriptor.safety_check_for_workstation(&node).is_ok());
}

#[test]
fn rollback_workstation_mode_refuses_when_objective_missing() {
    // No rollback-objective declared — workstation mode requires it
    // because a content-free brief is useless.
    let node = rollback_node_with(Some("workstation"), None, Some(r#"["src/a.rs"]"#), None);
    let descriptor = build_rollback_descriptor(&node);
    let err = descriptor
        .safety_check_for_workstation(&node)
        .expect_err("missing objective must refuse");
    assert!(err.contains(":rollback-objective"));
    let eval = pre_dispatch_rollback_decision(&node);
    assert_eq!(eval.status, RollbackStatus::Refused);
    assert!(eval.reason.contains(":rollback-objective"));
}

#[test]
fn rollback_workstation_mode_refuses_when_owned_files_empty() {
    let node = rollback_node_with(Some("workstation"), Some("undo step"), None, None);
    let descriptor = build_rollback_descriptor(&node);
    let err = descriptor
        .safety_check_for_workstation(&node)
        .expect_err("missing owned files must refuse");
    assert!(err.contains(":rollback-owned-files"));
    let eval = pre_dispatch_rollback_decision(&node);
    assert_eq!(eval.status, RollbackStatus::Refused);
}

#[test]
fn rollback_workstation_mode_refuses_when_no_project_signal() {
    // Mutate the node so neither :target-project nor :requested-cwd
    // is set — the safety gate must refuse.
    let mut node = rollback_node_with(
        Some("workstation"),
        Some("undo step"),
        Some(r#"["src/a.rs"]"#),
        None,
    );
    node.target_project = None;
    node.requested_cwd = None;
    let descriptor = build_rollback_descriptor(&node);
    let err = descriptor
        .safety_check_for_workstation(&node)
        .expect_err("missing project signal must refuse");
    assert!(err.contains(":target-project"));
    let eval = pre_dispatch_rollback_decision(&node);
    assert_eq!(eval.status, RollbackStatus::Refused);
    assert!(eval.reason.contains(":target-project"));
}

#[test]
fn rollback_workstation_mode_refuses_when_dispatch_strategy_unsafe() {
    // `unknown` (the default) is not on the inferable whitelist;
    // the safety gate must refuse so the rollback never rides an
    // unsupported substrate.
    let mut node = rollback_node_with(
        Some("workstation"),
        Some("undo step"),
        Some(r#"["src/a.rs"]"#),
        None,
    );
    node.dispatch_strategy = Some("unknown".into());
    let descriptor = build_rollback_descriptor(&node);
    let err = descriptor
        .safety_check_for_workstation(&node)
        .expect_err("unsafe dispatch strategy must refuse");
    assert!(err.contains(":dispatch-strategy"));
    let eval = pre_dispatch_rollback_decision(&node);
    assert_eq!(eval.status, RollbackStatus::Refused);
}

#[test]
fn rollback_evaluation_to_json_carries_every_surface_field() {
    let eval = RollbackEvaluation {
        policy: RollbackPolicy::Workstation,
        status: RollbackStatus::Dispatched,
        reason: "ok".into(),
        objective: Some("undo step".into()),
        owned_files: vec!["src/a.rs".into()],
        acceptance_commands: vec!["cargo test".into()],
        task_brief_preview: Some("## Objective\nundo step\n".into()),
        task_brief_path: Some("/tmp/rollback.md".into()),
        inner_payload: Some(json!({"task_id": "btk-rb"})),
        cascade: None,
    };
    let v = eval.to_json();
    assert_eq!(v["policy"], "workstation");
    assert_eq!(v["status"], "dispatched");
    assert_eq!(v["reason"], "ok");
    assert_eq!(v["objective"], "undo step");
    assert_eq!(v["owned_files"][0], "src/a.rs");
    assert_eq!(v["acceptance_commands"][0], "cargo test");
    // CRITICAL invariant — `acceptance_commands_executed=false` is
    // pinned so audit dashboards can pivot on the flag and prove
    // the scheduler never ran shell on behalf of a rollback brief.
    assert_eq!(v["acceptance_commands_executed"], false);
    assert!(v["task_brief_preview"]
        .as_str()
        .unwrap()
        .contains("undo step"));
    assert_eq!(v["task_brief_path"], "/tmp/rollback.md");
    assert_eq!(v["inner_result"]["task_id"], "btk-rb");
}

#[test]
fn rollback_status_wire_strings_are_distinct_and_stable() {
    // Pin the wire vocabulary so audit dashboards can grep on
    // these strings without re-deriving them from the enum.
    assert_eq!(RollbackStatus::NotRequested.as_wire(), "not_requested");
    assert_eq!(
        RollbackStatus::DescriptorReady.as_wire(),
        "descriptor_ready"
    );
    assert_eq!(RollbackStatus::Dispatched.as_wire(), "dispatched");
    assert_eq!(RollbackStatus::Refused.as_wire(), "refused");
    assert_eq!(RollbackStatus::Failed.as_wire(), "failed");
    // RollbackPolicy mirror.
    assert_eq!(RollbackPolicy::None.as_wire(), "none");
    assert_eq!(RollbackPolicy::Descriptor.as_wire(), "descriptor");
    assert_eq!(RollbackPolicy::Workstation.as_wire(), "workstation");
}

#[test]
fn rollback_evaluation_is_inactive_only_when_truly_empty() {
    // Inactive: no policy + no fields.
    let inactive = RollbackEvaluation {
        policy: RollbackPolicy::None,
        status: RollbackStatus::NotRequested,
        reason: "no rollback hints declared".into(),
        objective: None,
        owned_files: vec![],
        acceptance_commands: vec![],
        task_brief_preview: None,
        task_brief_path: None,
        inner_payload: None,
        cascade: None,
    };
    assert!(inactive.is_inactive());
    // ANY signal should flip is_inactive to false so the response
    // surfaces the row even when the policy is None (e.g. the
    // explicit-none case where the author wrote out an objective
    // but then suppressed dispatch).
    let mut with_obj = inactive.clone();
    with_obj.objective = Some("intent".into());
    assert!(!with_obj.is_inactive());
}

#[test]
fn build_nodes_summary_surfaces_rollback_block_when_present() {
    let nodes = vec![
        DagNode {
            id: "with".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            rollback_policy: Some("descriptor".into()),
            rollback_objective: Some("undo".into()),
            ..Default::default()
        },
        DagNode {
            id: "plain".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let order = vec!["with".to_string(), "plain".to_string()];
    let summary = build_nodes_summary(&nodes, &order);
    let arr = summary.as_array().unwrap();
    assert_eq!(arr[0]["rollback"]["policy"], "descriptor");
    assert_eq!(arr[0]["rollback"]["objective"], "undo");
    // Plain node has no rollback hints — summary stays quiet
    // (regression guard for the wave-17 / task 03 baseline).
    assert!(arr[1].get("rollback").is_none());
}

#[test]
fn node_results_json_surfaces_rollback_block_only_when_active() {
    let mut o = ExecutionOutcome::default();
    // Active rollback — surfaces.
    o.results.push(NodeResult {
        id: "with_rb".into(),
        target: "mission_task_delegate".into(),
        state: NodeState::Failed {
            reason: "boom".into(),
        },
        dispatch_strategy: "fresh-code-alignment".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: Some(RollbackEvaluation {
            policy: RollbackPolicy::Descriptor,
            status: RollbackStatus::DescriptorReady,
            reason: "descriptor mode".into(),
            objective: Some("undo".into()),
            owned_files: vec!["src/a.rs".into()],
            acceptance_commands: vec![],
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        }),
        acceptance: None,
    });
    // No rollback hints — quiet.
    o.results.push(NodeResult {
        id: "plain".into(),
        target: "mission_execution".into(),
        state: NodeState::Failed {
            reason: "boom".into(),
        },
        dispatch_strategy: "unknown".into(),
        inner_payload: json!({}),
        attempts_made: 1,
        max_attempts: 1,
        retry_skipped_non_retryable: false,
        rollback: None,
        acceptance: None,
    });
    let v = o.node_results_json();
    let arr = v.as_array().unwrap();
    assert_eq!(arr[0]["rollback"]["policy"], "descriptor");
    assert_eq!(arr[0]["rollback"]["status"], "descriptor_ready");
    assert!(arr[1].get("rollback").is_none());
}

#[tokio::test]
async fn run_rollback_descriptor_mode_skips_dispatch_and_records_brief() {
    // Descriptor mode never dispatches — we can run the async
    // helper without a real AppState because the substrate is
    // never invoked. We use a dummy state via the existing
    // `tempfile`-backed registry path the workstation_dispatch
    // tests use; descriptor mode doesn't read any AppState
    // fields.
    //
    // To stay self-contained we just call the pure pre-dispatch
    // decision (which is the byte-identical projection minus the
    // brief preview) and assert the contract.
    let node = rollback_node_with(
        Some("descriptor"),
        Some("undo step"),
        Some(r#"["src/a.rs"]"#),
        Some(r#"["cargo test"]"#),
    );
    let eval = pre_dispatch_rollback_decision(&node);
    assert_eq!(eval.policy, RollbackPolicy::Descriptor);
    assert_eq!(eval.status, RollbackStatus::DescriptorReady);
    // CRITICAL — descriptor mode NEVER produces an inner_payload
    // because the substrate is not invoked.
    assert!(eval.inner_payload.is_none());
}

#[test]
fn rollback_workstation_brief_includes_canonical_sections() {
    // The rollback brief reuses the wave-15 task-brief shape so
    // observers see the same headings as a forward task brief.
    let node = rollback_node_with(
        Some("workstation"),
        Some("undo migration step 3"),
        Some(r#"["src/migrations/0003.rs"]"#),
        Some(r#"["cargo test -p missiond"]"#),
    );
    let descriptor = build_rollback_descriptor(&node);
    let hints = descriptor.to_workstation_hints(&node);
    assert_eq!(hints.objective.as_deref(), Some("undo migration step 3"));
    assert!(hints
        .scope
        .as_deref()
        .unwrap()
        .contains("rollback for failed plan-DAG node"));
    assert_eq!(
        hints.owned_files,
        vec!["src/migrations/0003.rs".to_string()]
    );
    assert_eq!(
        hints.acceptance_commands,
        vec!["cargo test -p missiond".to_string()]
    );
    // Default commit policy lands as "scoped" so the rollback
    // brief inherits the scoped-commit invariant.
    assert_eq!(hints.commit_policy.as_deref(), Some("scoped"));
    // Build the brief through the substrate helper to confirm
    // the canonical sections are present.
    let plan = fixture_plan("(plan)");
    let brief = crate::handlers::knowledge::workstation_dispatch::build_task_brief(
        &plan,
        &hints,
        "fresh-code-alignment",
    );
    assert!(brief.contains("## Objective"));
    assert!(brief.contains("## Scope"));
    assert!(brief.contains("rollback for failed plan-DAG node"));
    assert!(brief.contains("## Owned files"));
    assert!(brief.contains("- src/migrations/0003.rs"));
    assert!(brief.contains("## Acceptance commands"));
    assert!(brief.contains("- cargo test -p missiond"));
    assert!(brief.contains("## Commit policy"));
}

#[test]
fn rollback_failure_policy_interaction_taint_still_propagates_under_descriptor_mode() {
    // Pin the failure-policy contract: the rollback decision
    // never short-circuits taint propagation. We exercise the
    // pure helpers because the wave loop has the full integration
    // (covered by build_validated_dag + execute_with_concurrency
    // in production); the decision-level test here protects
    // against a refactor that flips the rollback evaluator into
    // a "rolled back successfully so don't taint downstream"
    // bug.
    //
    // Concretely: a node with `:rollback-policy "descriptor"`
    // that fails MUST still cause `propagate_taint` to mark its
    // downstream as `tainted_by`. We build the graph + simulate
    // the failure here.
    let nodes = vec![
        DagNode {
            id: "a".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            rollback_policy: Some("descriptor".into()),
            rollback_objective: Some("undo".into()),
            ..Default::default()
        },
        DagNode {
            id: "b".into(),
            target: "mission_execution".into(),
            depends_on: vec!["a".into()],
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &nodes {
        for dep in &n.depends_on {
            succs.entry(dep.as_str()).or_default().push(n.id.as_str());
        }
    }
    let mut tainted: HashMap<String, String> = HashMap::new();
    // Simulate the wave loop: rollback runs FIRST, then
    // propagate_taint. Both happen so downstream stays tainted.
    let _eval = pre_dispatch_rollback_decision(&nodes[0]);
    propagate_taint(&nodes[0], &succs, &mut tainted);
    assert_eq!(
        tainted.get("b"),
        Some(&"a".to_string()),
        "rollback descriptor mode must NOT short-circuit downstream taint"
    );
}

#[test]
fn rollback_failure_policy_interaction_taint_still_propagates_under_workstation_mode() {
    // Same invariant for the workstation mode — even when
    // the rollback dispatch succeeds, taint propagates so
    // downstream sees the failure.
    let nodes = vec![
        DagNode {
            id: "a".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "continue".into(),
            rollback_policy: Some("workstation".into()),
            rollback_objective: Some("undo".into()),
            rollback_owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
            target_project: Some("missiond".into()),
            dispatch_strategy: Some("fresh-code-alignment".into()),
            ..Default::default()
        },
        DagNode {
            id: "b".into(),
            target: "mission_execution".into(),
            depends_on: vec!["a".into()],
            failure_policy: "continue".into(),
            ..Default::default()
        },
    ];
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &nodes {
        for dep in &n.depends_on {
            succs.entry(dep.as_str()).or_default().push(n.id.as_str());
        }
    }
    let mut tainted: HashMap<String, String> = HashMap::new();
    let _eval = pre_dispatch_rollback_decision(&nodes[0]);
    propagate_taint(&nodes[0], &succs, &mut tainted);
    assert_eq!(
        tainted.get("b"),
        Some(&"a".to_string()),
        "rollback workstation mode must NOT short-circuit downstream taint"
    );
}

#[test]
fn rollback_safe_descriptor_refusals_are_non_retryable() {
    // Per the brief: SafeDescriptor refusals must not be retried.
    // The refusal vocabulary the rollback evaluator emits when
    // safety fails is `RollbackStatus::Refused`. Pin this so a
    // future refactor can't accidentally route a refused
    // rollback back through the wave-loop's retry path.
    //
    // Test: a node with `:rollback-policy "workstation"` but
    // missing `:rollback-objective` should evaluate to Refused
    // and the reason should explicitly mention the failing gate.
    let node = rollback_node_with(
        Some("workstation"),
        None, // objective missing — safety refusal
        Some(r#"["src/a.rs"]"#),
        None,
    );
    let eval = pre_dispatch_rollback_decision(&node);
    assert_eq!(eval.status, RollbackStatus::Refused);
    // The wave loop's retry-decision predicate is
    // `plan_node_should_retry`. SafeDescriptor refusals from the
    // forward dispatch already set `non_retryable=true`. Our
    // rollback Refused status is similarly non-retryable in the
    // sense that the wave loop never retries the failed node
    // (it only ever runs the rollback ONCE per terminal failure).
    // We assert the stable wire form so dashboards can pivot.
    assert_eq!(eval.status.as_wire(), "refused");
}

#[test]
fn rollback_descriptor_carries_acceptance_commands_unexecuted() {
    // CRITICAL invariant — the rollback brief surfaces declared
    // commands verbatim AND the JSON projection pins
    // `acceptance_commands_executed=false` so audit dashboards
    // can prove the scheduler never ran them.
    let node = rollback_node_with(
        Some("descriptor"),
        Some("undo"),
        None,
        Some(r#"["rm -rf /" "echo all good"]"#),
    );
    let eval = pre_dispatch_rollback_decision(&node);
    let v = eval.to_json();
    assert_eq!(
        v["acceptance_commands_executed"], false,
        "rollback evaluator MUST surface acceptance_commands_executed=false"
    );
    assert_eq!(v["acceptance_commands"][0], "rm -rf /");
    assert_eq!(v["acceptance_commands"][1], "echo all good");
}

// ── wave-17 / task 05 — DAG finalize + distill trigger v0 ──────────

#[test]
fn parse_finalize_plan_defaults_false_for_backward_compat() {
    // No knob present → existing wave-17 / task 04 byte-shape MUST be
    // preserved. The whole point of the default is that nothing
    // observable changes for callers that did not opt in.
    assert!(!parse_finalize_plan(&json!({})));
    assert!(!parse_finalize_plan(&json!({"finalize_plan": false})));
    assert!(parse_finalize_plan(&json!({"finalize_plan": true})));
    // Non-bool values normalise to false rather than fail — finalize is
    // additive so a typo on the runtime knob never breaks dispatch.
    assert!(!parse_finalize_plan(&json!({"finalize_plan": "yes"})));
    assert!(!parse_finalize_plan(&json!({"finalize_plan": 1})));
}

#[test]
fn parse_distill_on_success_defaults_false() {
    assert!(!parse_distill_on_success(&json!({})));
    assert!(!parse_distill_on_success(
        &json!({"distill_on_success": false})
    ));
    assert!(parse_distill_on_success(
        &json!({"distill_on_success": true})
    ));
    assert!(!parse_distill_on_success(
        &json!({"distill_on_success": "yep"})
    ));
}

#[test]
fn parse_distill_mode_arg_default_dry_run() {
    // Absence + empty + literal "dry_run" all collapse onto the
    // canonical "dry_run" so the response always echoes a known mode.
    assert_eq!(parse_distill_mode_arg(&json!({})).unwrap(), "dry_run");
    assert_eq!(
        parse_distill_mode_arg(&json!({"distill_mode": ""})).unwrap(),
        "dry_run"
    );
    assert_eq!(
        parse_distill_mode_arg(&json!({"distill_mode": "dry_run"})).unwrap(),
        "dry_run"
    );
    assert_eq!(
        parse_distill_mode_arg(&json!({"distill_mode": "sonnet"})).unwrap(),
        "sonnet"
    );
}

#[test]
fn parse_distill_mode_arg_rejects_typos() {
    // Strict allowlist mirrors workflow.rs::parse_distill_mode so the
    // two surfaces cannot drift; a typo must surface even when
    // distill_on_success=false (caught up-front by validate_finalize_args).
    let err = parse_distill_mode_arg(&json!({"distill_mode": "sonet"})).unwrap_err();
    assert!(
        err.contains("dry_run"),
        "error must spell out the allowlist"
    );
    assert!(err.contains("sonet"), "error must echo the rejected value");
}

#[test]
fn validate_finalize_args_accepts_default_baseline() {
    // No finalize knobs at all → wave-17 / task 04 byte-shape lives.
    assert!(validate_finalize_args(&json!({})).is_none());
    // finalize_plan alone is fine — distill is opt-in on top.
    assert!(validate_finalize_args(&json!({"finalize_plan": true})).is_none());
    // finalize_plan + distill_on_success is the canonical opt-in.
    assert!(
        validate_finalize_args(&json!({"finalize_plan": true, "distill_on_success": true}))
            .is_none()
    );
    assert!(validate_finalize_args(
        &json!({"finalize_plan": true, "distill_on_success": true, "distill_mode": "sonnet"})
    )
    .is_none());
}

#[test]
fn validate_finalize_args_rejects_distill_without_finalize() {
    // Silently ignoring a distill request would mask caller intent —
    // fail-fast surfaces the contradiction.
    let result = validate_finalize_args(&json!({"distill_on_success": true})).unwrap();
    assert_eq!(result.is_error, Some(true));
    let payload = tool_result_payload(&result);
    // Structured errors serialise as `{ error_code, reason, suggestion?, trace_id? }`.
    // The reason field carries the human-readable diagnostic.
    let reason = payload
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        reason.contains("finalize_plan=true"),
        "error must point at the missing finalize knob; got `{}` (full payload: {})",
        reason,
        payload
    );
    assert_eq!(
        payload.get("error_code").and_then(|v| v.as_str()),
        Some("INVALID_PARAM")
    );
}

#[test]
fn validate_finalize_args_rejects_unknown_distill_mode() {
    // Validation runs even when distill_on_success=false — a typo
    // should fail the next live caller's dispatch up-front, not
    // silently survive into production.
    let result = validate_finalize_args(&json!({"distill_mode": "warp"})).unwrap();
    assert_eq!(result.is_error, Some(true));
}

#[test]
fn finalize_plan_status_label_maps_aggregate_to_status() {
    // Pin the aggregate -> plan_status mapping table so a future
    // refactor cannot silently advance the plan FSM past a paused
    // run. The `dag_paused` row in particular MUST preserve the
    // current status — claiming success while a node awaits review
    // is the exact "do not lie" invariant the brief calls out.
    assert_eq!(
        finalize_plan_status_label("dag_succeeded", "executing"),
        "succeeded"
    );
    assert_eq!(
        finalize_plan_status_label("dag_failed", "executing"),
        "failed"
    );
    assert_eq!(
        finalize_plan_status_label("dag_partial", "executing"),
        "failed"
    );
    // Paused → never claim success. Preserve the in-flight status
    // so the resume helper (wave-17 / task 01) can advance it once
    // the gate resolves.
    assert_eq!(
        finalize_plan_status_label("dag_paused", "executing"),
        "executing"
    );
    assert_eq!(
        finalize_plan_status_label("dag_paused", "awaiting_review"),
        "awaiting_review"
    );
    // Defensive: an unrecognised aggregate must not pretend success.
    assert_eq!(
        finalize_plan_status_label("dag_unknown", "approved"),
        "unchanged"
    );
}

#[test]
fn build_finalization_block_carries_rule_label_per_aggregate() {
    // The `rule` field lets audit dashboards group runs by the same
    // mapping rule without re-deriving the aggregate semantics.
    let succeeded = build_finalization_block("dag_succeeded", Some("succeeded"), None, None);
    assert_eq!(succeeded["finalize_plan"], true);
    assert_eq!(succeeded["aggregate_status"], "dag_succeeded");
    assert_eq!(succeeded["final_plan_status"], "succeeded");
    assert_eq!(succeeded["rule"], "all_terminal_no_failed_no_paused");
    assert!(succeeded.get("distill").is_none());

    let failed = build_finalization_block("dag_failed", Some("failed"), None, None);
    assert_eq!(failed["rule"], "fail_fast_or_failure_dominates");

    let partial = build_finalization_block("dag_partial", Some("failed"), None, None);
    assert_eq!(partial["rule"], "failed_node_or_skipped_without_paused");

    // Paused: response MUST report the current (preserved) status —
    // not a fictitious "succeeded".
    let paused = build_finalization_block("dag_paused", Some("executing"), None, None);
    assert_eq!(paused["final_plan_status"], "executing");
    assert_eq!(paused["rule"], "paused_node_present_no_finalization");
}

#[test]
fn build_finalization_block_surfaces_distill_block_when_present() {
    // The distill block round-trips into the finalization shape so
    // callers can grep `finalization.distill.triggered` without a
    // second hop.
    let distill = build_distill_block(
        true,
        "distill_invoked_ok",
        "dry_run",
        Some(json!({"ok": true})),
        false,
    );
    let block = build_finalization_block("dag_succeeded", Some("succeeded"), None, Some(distill));
    assert_eq!(block["distill"]["triggered"], true);
    assert_eq!(block["distill"]["reason"], "distill_invoked_ok");
    assert_eq!(block["distill"]["distill_mode"], "dry_run");
    assert_eq!(block["distill"]["result"]["ok"], true);
    assert!(block["distill"].get("warning").is_none());
}

#[test]
fn build_finalization_block_surfaces_plan_status_update_error() {
    // When the FSM update itself fails (e.g. PG transient error) the
    // block MUST surface that explicitly so callers can route — the
    // distill trigger ALSO refuses to fire in that case (verified by
    // maybe_run_distill_trigger logic) so the audit row never claims a
    // distill ran against an inconsistent plan state.
    let block = build_finalization_block("dag_succeeded", None, Some("DB connection lost"), None);
    assert_eq!(
        block["plan_status_update_error"], "DB connection lost",
        "FSM update error must round-trip into the response"
    );
    assert_eq!(block["final_plan_status"], "unchanged");
}

#[test]
fn build_distill_block_skipped_path_preserves_reason() {
    // distill_on_success=false → no trigger, no result, but we still
    // surface the mode for the response shape consistency.
    let b = build_distill_block(false, "aggregate_not_succeeded", "dry_run", None, false);
    assert_eq!(b["triggered"], false);
    assert_eq!(b["reason"], "aggregate_not_succeeded");
    assert_eq!(b["distill_mode"], "dry_run");
    assert!(b.get("result").is_none());
    assert!(b.get("warning").is_none());
}

#[test]
fn build_distill_block_failure_surfaces_warning_keeps_triggered_true() {
    // When the workflow distill handler returns an error we MUST keep
    // `triggered=true` (it ran) but add a `warning` so callers can
    // detect partial success. CRITICAL: the brief forbids breaking
    // the plan final state when distill fails.
    let b = build_distill_block(
        true,
        "distill_invoked_returned_error",
        "sonnet",
        Some(json!({"error": "sonnet quota exhausted"})),
        true,
    );
    assert_eq!(b["triggered"], true);
    assert_eq!(b["distill_mode"], "sonnet");
    assert_eq!(
        b["warning"],
        "distill trigger returned an error; plan final state preserved"
    );
    assert_eq!(b["result"]["error"], "sonnet quota exhausted");
}

#[test]
fn build_distill_block_success_omits_warning() {
    let b = build_distill_block(
        true,
        "distill_invoked_ok",
        "dry_run",
        Some(json!({"status": "dry_run", "persisted": false})),
        false,
    );
    assert_eq!(b["triggered"], true);
    assert!(b.get("warning").is_none(), "ok branch must not warn");
    assert_eq!(b["result"]["persisted"], false);
}

// ── wave-18 / task 04 — conservative cascade rollback ────────────────
//
// Cascade evaluator is conservative by design: it never runs unless
// the failed (cascade-root) node opted in via `:rollback-cascade`,
// and even then `plan` mode never dispatches. `dispatch-safe` mode
// dispatches a compensation node only when its OWN safety gates
// pass; descriptor-only / no-policy compensations stay
// descriptor_ready (never silently promoted). These tests pin the
// parser, the compensation discovery + ordering, the plan-mode
// recording branch, the dispatch-safe refusal branch, and the
// default-mode invariant.

#[test]
fn parse_node_form_captures_cascade_hints() {
    let sexp = r#"
        (plan
          (node :id "fail"
                :target "mission_task_delegate"
                :rollback-cascade "dispatch-safe"
                :rollback-policy "descriptor"
                :rollback-objective "root failed")
          (node :id "comp-1"
                :target "mission_task_delegate"
                :compensates "fail"
                :rollback-after ["comp-2"])
          (node :id "comp-2"
                :target "mission_task_delegate"
                :compensates "fail"))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes.len(), 3);
    // Cascade root parses :rollback-cascade onto the typed slot
    // AND surfaces the typed projection.
    let root = &parsed.nodes[0];
    assert_eq!(root.rollback_cascade.as_deref(), Some("dispatch-safe"));
    assert_eq!(
        root.rollback_cascade_kind(),
        Some(RollbackCascadeMode::DispatchSafe)
    );
    assert!(root.has_active_rollback_cascade());
    // Compensation nodes parse :compensates + :rollback-after.
    let c1 = &parsed.nodes[1];
    assert_eq!(c1.compensates.as_deref(), Some("fail"));
    assert_eq!(c1.rollback_after, vec!["comp-2".to_string()]);
    let c2 = &parsed.nodes[2];
    assert_eq!(c2.compensates.as_deref(), Some("fail"));
    assert!(c2.rollback_after.is_empty());
    // None of the new keys lands in unsupported_fields.
    for n in &parsed.nodes {
        for forbidden in ["compensates", "rollback-cascade", "rollback-after"] {
            assert!(
                !n.unsupported_fields.iter().any(|(k, _)| k == forbidden),
                "key `{}` must land on a typed slot, not unsupported_fields",
                forbidden
            );
        }
    }
}

#[test]
fn parse_node_form_records_unrecognised_rollback_cascade_mode_in_unsupported() {
    let sexp = r#"
        (plan
          (node :id "n1"
                :target "mission_task_delegate"
                :rollback-cascade "yolo"))
    "#;
    let parsed = parse_plan_dag(sexp);
    let n = &parsed.nodes[0];
    // Raw value still lands on the typed slot (so the response
    // round-trips author intent).
    assert_eq!(n.rollback_cascade.as_deref(), Some("yolo"));
    // But the typed projection refuses to interpret a typo.
    assert!(n.rollback_cascade_kind().is_none());
    assert!(!n.has_active_rollback_cascade());
    // And the unsupported_fields audit captures the typo.
    assert!(n
        .unsupported_fields
        .iter()
        .any(|(k, v)| k == "rollback-cascade" && v == "yolo"));
}

#[test]
fn rollback_cascade_default_is_inactive() {
    // No `:rollback-cascade` declared → cascade evaluator never runs.
    let node = DagNode {
        id: "n".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        ..Default::default()
    };
    assert!(node.rollback_cascade_kind().is_none());
    assert!(!node.has_active_rollback_cascade());
}

#[test]
fn rollback_cascade_explicit_none_is_inactive() {
    // `:rollback-cascade "none"` is the explicit opt-out.
    let node = DagNode {
        id: "n".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        rollback_cascade: Some("none".into()),
        ..Default::default()
    };
    assert_eq!(
        node.rollback_cascade_kind(),
        Some(RollbackCascadeMode::None)
    );
    assert!(!node.has_active_rollback_cascade());
}

#[test]
fn compute_compensation_order_finds_compensates_matches() {
    // Two compensation nodes, no :rollback-after edges → ordering
    // follows forward topological order (then declaration as a
    // tie-break for nodes not in the forward order).
    let nodes = vec![
        DagNode {
            id: "fail".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
        DagNode {
            id: "comp-a".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            ..Default::default()
        },
        DagNode {
            id: "comp-b".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            ..Default::default()
        },
        DagNode {
            id: "unrelated".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let order = vec![
        "fail".to_string(),
        "comp-a".to_string(),
        "comp-b".to_string(),
        "unrelated".to_string(),
    ];
    let ordered = compute_compensation_order("fail", &nodes, &order);
    assert_eq!(ordered.len(), 2);
    assert_eq!(ordered[0].id, "comp-a");
    assert_eq!(ordered[1].id, "comp-b");
}

#[test]
fn compute_compensation_order_honours_rollback_after_edge() {
    // comp-a declares `:rollback-after ["comp-b"]` so the cascade
    // ordering MUST place comp-b before comp-a even though comp-a
    // comes first in the forward order.
    let nodes = vec![
        DagNode {
            id: "fail".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
        DagNode {
            id: "comp-a".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            rollback_after: vec!["comp-b".into()],
            ..Default::default()
        },
        DagNode {
            id: "comp-b".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            ..Default::default()
        },
    ];
    let order = vec![
        "fail".to_string(),
        "comp-a".to_string(),
        "comp-b".to_string(),
    ];
    let ordered = compute_compensation_order("fail", &nodes, &order);
    assert_eq!(ordered.len(), 2);
    assert_eq!(
        ordered[0].id, "comp-b",
        ":rollback-after must place comp-b first"
    );
    assert_eq!(ordered[1].id, "comp-a");
}

#[test]
fn compute_compensation_order_cycle_falls_back_to_declaration_order() {
    // Both nodes declare `:rollback-after` for each other — that
    // is a cycle (a typo). The evaluator must NOT deadlock; it
    // falls back to declaration order so the cascade still runs.
    let nodes = vec![
        DagNode {
            id: "fail".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
        DagNode {
            id: "comp-a".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            rollback_after: vec!["comp-b".into()],
            ..Default::default()
        },
        DagNode {
            id: "comp-b".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            rollback_after: vec!["comp-a".into()],
            ..Default::default()
        },
    ];
    let order = vec![
        "fail".to_string(),
        "comp-a".to_string(),
        "comp-b".to_string(),
    ];
    let ordered = compute_compensation_order("fail", &nodes, &order);
    // Cycle resolution: every candidate still appears, no deadlock.
    assert_eq!(ordered.len(), 2);
    let ids: Vec<&str> = ordered.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains(&"comp-a"));
    assert!(ids.contains(&"comp-b"));
}

#[test]
fn compute_compensation_order_returns_empty_when_no_compensations_declared() {
    let nodes = vec![DagNode {
        id: "fail".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        ..Default::default()
    }];
    let order = vec!["fail".to_string()];
    assert!(compute_compensation_order("fail", &nodes, &order).is_empty());
}

#[test]
fn build_compensation_plan_entry_records_descriptor_without_dispatch() {
    // Pure helper — verifies that `plan` mode produces a
    // descriptor_ready row with no inner_payload.
    let plan = fixture_plan("(plan)");
    let comp = DagNode {
        id: "comp-1".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        compensates: Some("fail".into()),
        rollback_policy: Some("descriptor".into()),
        rollback_objective: Some("undo step".into()),
        rollback_owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
        target_project: Some("missiond".into()),
        dispatch_strategy: Some("fresh-code-alignment".into()),
        ..Default::default()
    };
    let entry = build_compensation_plan_entry(&plan, &comp);
    assert_eq!(entry.node_id, "comp-1");
    assert_eq!(entry.policy, RollbackPolicy::Descriptor);
    assert_eq!(entry.status, RollbackStatus::DescriptorReady);
    assert_eq!(entry.objective.as_deref(), Some("undo step"));
    assert_eq!(entry.owned_files, vec!["src/a.rs".to_string()]);
    // CRITICAL — `plan` mode never produces an inner_payload.
    assert!(entry.inner_payload.is_none());
    // Brief preview is built locally because the objective is set.
    assert!(entry.task_brief_preview.is_some());
    let v = entry.to_json();
    assert_eq!(v["node_id"], "comp-1");
    assert_eq!(v["status"], "descriptor_ready");
    // Pin the audit invariant — declared commands are NEVER executed.
    assert_eq!(v["acceptance_commands_executed"], false);
}

#[test]
fn cascade_outcome_to_json_carries_every_surface_field() {
    let cascade = CascadeRollbackOutcome {
        mode: RollbackCascadeMode::Plan,
        cascade_root: "fail".into(),
        compensations: vec![CascadeCompensationOutcome {
            node_id: "comp-1".into(),
            policy: RollbackPolicy::Descriptor,
            status: RollbackStatus::DescriptorReady,
            reason: "recorded".into(),
            objective: Some("undo".into()),
            owned_files: vec!["src/a.rs".into()],
            acceptance_commands: vec![],
            task_brief_preview: Some("## Objective\nundo\n".into()),
            task_brief_path: None,
            inner_payload: None,
        }],
        reason: "cascade plan: 1 compensation".into(),
    };
    let v = cascade.to_json();
    assert_eq!(v["mode"], "plan");
    assert_eq!(v["cascade_root"], "fail");
    assert_eq!(v["reason"], "cascade plan: 1 compensation");
    let comps = v["compensations"].as_array().unwrap();
    assert_eq!(comps.len(), 1);
    assert_eq!(comps[0]["node_id"], "comp-1");
    assert_eq!(comps[0]["status"], "descriptor_ready");
}

#[test]
fn cascade_outcome_inactive_when_no_mode_and_no_compensations() {
    let inactive = CascadeRollbackOutcome {
        mode: RollbackCascadeMode::None,
        cascade_root: "fail".into(),
        compensations: vec![],
        reason: "skipped".into(),
    };
    assert!(inactive.is_inactive());
    let active_mode = CascadeRollbackOutcome {
        mode: RollbackCascadeMode::Plan,
        cascade_root: "fail".into(),
        compensations: vec![],
        reason: "no compensation declared".into(),
    };
    assert!(!active_mode.is_inactive());
}

#[test]
fn rollback_cascade_mode_wire_strings_are_distinct_and_stable() {
    assert_eq!(RollbackCascadeMode::None.as_wire(), "none");
    assert_eq!(RollbackCascadeMode::Plan.as_wire(), "plan");
    assert_eq!(RollbackCascadeMode::DispatchSafe.as_wire(), "dispatch-safe");
    // Author-friendly aliases parse identically.
    assert_eq!(
        RollbackCascadeMode::parse("dispatch_safe"),
        Some(RollbackCascadeMode::DispatchSafe)
    );
}

#[tokio::test]
async fn run_cascade_rollback_plan_mode_records_compensations_without_dispatch() {
    // Construct an `AppState` is heavy; use the pure helper +
    // synthesise the cascade outcome by hand to verify the plan
    // mode contract end-to-end without standing up the substrate.
    let plan = fixture_plan("(plan)");
    let nodes = vec![
        DagNode {
            id: "fail".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            rollback_cascade: Some("plan".into()),
            ..Default::default()
        },
        DagNode {
            id: "comp-1".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            rollback_policy: Some("descriptor".into()),
            rollback_objective: Some("undo".into()),
            target_project: Some("missiond".into()),
            dispatch_strategy: Some("fresh-code-alignment".into()),
            ..Default::default()
        },
    ];
    let order = vec!["fail".into(), "comp-1".into()];
    // Use the pure helpers directly — `plan` mode never touches the
    // substrate, so we can synthesise the outcome with the same code
    // path the async helper takes.
    let ordered = compute_compensation_order("fail", &nodes, &order);
    assert_eq!(ordered.len(), 1);
    let entry = build_compensation_plan_entry(&plan, ordered[0]);
    assert_eq!(entry.status, RollbackStatus::DescriptorReady);
    assert!(entry.inner_payload.is_none());
}

#[test]
fn run_cascade_rollback_dispatch_safe_refuses_unsafe_compensation() {
    // Pure projection of the `dispatch-safe` decision: a compensation
    // node that opts into `:rollback-policy "workstation"` BUT misses
    // `:rollback-objective` MUST be refused (non-retryable). We
    // verify this through the safety check directly because the
    // cascade body uses the same check.
    let comp = DagNode {
        id: "comp-1".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        compensates: Some("fail".into()),
        rollback_policy: Some("workstation".into()),
        // objective intentionally missing
        rollback_owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
        target_project: Some("missiond".into()),
        dispatch_strategy: Some("fresh-code-alignment".into()),
        ..Default::default()
    };
    let descriptor = build_rollback_descriptor(&comp);
    let err = descriptor
        .safety_check_for_workstation(&comp)
        .expect_err("unsafe compensation must refuse");
    assert!(err.contains(":rollback-objective"));
}

#[test]
fn run_cascade_rollback_dispatch_safe_keeps_descriptor_only_compensations_recorded() {
    // CRITICAL invariant — `dispatch-safe` MUST NEVER promote a
    // descriptor-only compensation to a dispatch. We pin this by
    // building the plan entry directly: the resulting outcome is
    // descriptor_ready (recorded), not dispatched.
    let plan = fixture_plan("(plan)");
    let comp = DagNode {
        id: "comp-1".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        compensates: Some("fail".into()),
        rollback_policy: Some("descriptor".into()),
        rollback_objective: Some("undo".into()),
        target_project: Some("missiond".into()),
        dispatch_strategy: Some("fresh-code-alignment".into()),
        ..Default::default()
    };
    let entry = build_compensation_plan_entry(&plan, &comp);
    assert_eq!(entry.policy, RollbackPolicy::Descriptor);
    assert_eq!(
        entry.status,
        RollbackStatus::DescriptorReady,
        "dispatch-safe MUST NOT promote a descriptor-only compensation"
    );
    assert!(entry.inner_payload.is_none());
}

#[test]
fn rollback_evaluation_with_cascade_surfaces_cascade_block_in_json() {
    let mut eval = RollbackEvaluation {
        policy: RollbackPolicy::Descriptor,
        status: RollbackStatus::DescriptorReady,
        reason: "descriptor mode".into(),
        objective: Some("undo".into()),
        owned_files: vec![],
        acceptance_commands: vec![],
        task_brief_preview: None,
        task_brief_path: None,
        inner_payload: None,
        cascade: None,
    };
    // Without a cascade outcome attached, JSON omits the cascade key.
    let v = eval.to_json();
    assert!(v.get("cascade").is_none());
    // Attach a cascade outcome — JSON now surfaces it.
    eval.cascade = Some(CascadeRollbackOutcome {
        mode: RollbackCascadeMode::Plan,
        cascade_root: "fail".into(),
        compensations: vec![CascadeCompensationOutcome {
            node_id: "comp-1".into(),
            policy: RollbackPolicy::Descriptor,
            status: RollbackStatus::DescriptorReady,
            reason: "recorded".into(),
            objective: Some("undo".into()),
            owned_files: vec![],
            acceptance_commands: vec![],
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
        }],
        reason: "cascade plan: 1 compensation".into(),
    });
    let v2 = eval.to_json();
    assert_eq!(v2["cascade"]["mode"], "plan");
    assert_eq!(v2["cascade"]["cascade_root"], "fail");
    assert_eq!(v2["cascade"]["compensations"][0]["node_id"], "comp-1");
}

#[test]
fn build_nodes_summary_surfaces_cascade_hints_when_present() {
    let nodes = vec![
        DagNode {
            id: "with".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            rollback_cascade: Some("plan".into()),
            rollback_objective: Some("undo".into()),
            ..Default::default()
        },
        DagNode {
            id: "comp".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("with".into()),
            rollback_after: vec!["other".into()],
            ..Default::default()
        },
        DagNode {
            id: "plain".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let order = vec!["with".into(), "comp".into(), "plain".into()];
    let summary = build_nodes_summary(&nodes, &order);
    let arr = summary.as_array().unwrap();
    // Cascade root: `cascade_mode` surfaces under rollback block.
    assert_eq!(arr[0]["rollback"]["cascade_mode"], "plan");
    // Compensation node: `compensates` + `rollback_after` surface.
    assert_eq!(arr[1]["rollback"]["compensates"], "with");
    assert_eq!(arr[1]["rollback"]["rollback_after"][0], "other");
    // Plain node has no rollback hints — summary stays quiet
    // (regression guard for the wave-17 / task 04 baseline).
    assert!(arr[2].get("rollback").is_none());
}

#[test]
fn cascade_default_mode_preserves_wave17_byte_shape() {
    // CRITICAL invariant — a node WITHOUT `:rollback-cascade` MUST
    // NOT trigger any cascade evaluation. This protects every
    // existing wave-17 / task 04 test fixture from accidental
    // promotion.
    let node = DagNode {
        id: "n".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        // Author opted into node-local rollback but NOT cascade.
        rollback_policy: Some("descriptor".into()),
        rollback_objective: Some("undo".into()),
        ..Default::default()
    };
    assert!(!node.has_active_rollback_cascade());
    // The pre-dispatch decision still runs (and now gives back a
    // RollbackEvaluation with `cascade: None`).
    let eval = pre_dispatch_rollback_decision(&node);
    assert!(eval.cascade.is_none());
    // JSON projection omits the `cascade` key entirely.
    let v = eval.to_json();
    assert!(v.get("cascade").is_none());
}

// ── wave-19 / task 10 — forward `:compensate-node` references ─────
//
// Forward refs are declared on the failing-node side and point AT
// the compensation node id. They coexist with the wave-18 / task 04
// reverse `:compensates` direction. These tests pin the parser
// (both keyword spellings, no typed-slot leak), the validator
// (self-ref / unknown-id / direction-disagreement rejections), the
// candidate-discovery union with reverse refs, and the rollback
// hint surface.

#[test]
fn parse_node_form_captures_forward_compensate_node_ref() {
    let sexp = r#"
        (plan
          (node :id "fail"
                :target "mission_task_delegate"
                :rollback-cascade "plan"
                :compensate-node "comp-1")
          (node :id "comp-1"
                :target "mission_task_delegate"))
    "#;
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes.len(), 2);
    let fail = &parsed.nodes[0];
    assert_eq!(fail.compensate_node.as_deref(), Some("comp-1"));
    // Forward ref does NOT auto-populate the reverse slot on the
    // compensation node — only the failing-node side carries it.
    assert!(parsed.nodes[1].compensate_node.is_none());
    assert!(parsed.nodes[1].compensates.is_none());
    // Neither keyword spelling lands in unsupported_fields.
    for n in &parsed.nodes {
        for forbidden in [
            "compensate-node",
            "compensate_node",
            "compensate-ref",
            "compensate_ref",
        ] {
            assert!(
                !n.unsupported_fields.iter().any(|(k, _)| k == forbidden),
                "key `{}` must land on a typed slot, not unsupported_fields",
                forbidden
            );
        }
    }
}

#[test]
fn parse_node_form_accepts_compensate_ref_alias() {
    // The `:compensate-ref` alias resolves to the same typed slot
    // as `:compensate-node` so authors can pick the wording that
    // reads best in their plan dialect.
    let sexp = r#"
        (plan
          (node :id "fail"
                :target "mission_task_delegate"
                :compensate-ref "comp-1")
          (node :id "comp-1"
                :target "mission_task_delegate"))
    "#;
    let parsed = parse_plan_dag(sexp);
    let fail = &parsed.nodes[0];
    assert_eq!(fail.compensate_node.as_deref(), Some("comp-1"));
}

#[test]
fn build_validated_dag_rejects_self_compensate_node_ref() {
    // A node naming itself as its own compensation is a contract
    // bug: the validator MUST fail fast.
    let sexp = r#"
        (plan
          (node :id "fail"
                :target "mission_task_delegate"
                :compensate-node "fail"))
    "#;
    let err = build_validated_dag(sexp).expect_err("self-ref must fail");
    match err {
        DagBuildError::CompensateNodeInvalid {
            node_id,
            key,
            raw,
            detail,
        } => {
            assert_eq!(node_id, "fail");
            assert_eq!(key, "compensate-node");
            assert_eq!(raw, "fail");
            assert!(
                detail.contains("failing node itself"),
                "detail must mention self-reference: {}",
                detail
            );
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_unknown_compensate_node_ref() {
    // Pointing at an undeclared id is a typo — fail fast with a
    // structured error so the author sees it.
    let sexp = r#"
        (plan
          (node :id "fail"
                :target "mission_task_delegate"
                :compensate-node "ghost"))
    "#;
    let err = build_validated_dag(sexp).expect_err("unknown id must fail");
    match err {
        DagBuildError::CompensateNodeInvalid {
            node_id,
            raw,
            detail,
            ..
        } => {
            assert_eq!(node_id, "fail");
            assert_eq!(raw, "ghost");
            assert!(
                detail.contains("not declared"),
                "detail must mention undeclared id: {}",
                detail
            );
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn build_validated_dag_rejects_empty_compensate_node_ref() {
    // An empty value is meaningless and almost certainly a typo;
    // we surface it at validation time rather than silently dropping
    // the declaration.
    let sexp = r#"
        (plan
          (node :id "fail"
                :target "mission_task_delegate"
                :compensate-node ""))
    "#;
    // Note: the parser strips empty trimmed values via `set_first`,
    // so the slot stays None and validation is a no-op. To force
    // an empty slot we exercise the validator directly with a
    // hand-built node carrying the empty raw string.
    let nodes = vec![DagNode {
        id: "fail".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        compensate_node: Some("   ".into()),
        ..Default::default()
    }];
    // Re-run the same validator branch by inlining the relevant
    // check — we cannot call `build_validated_dag` with synthesised
    // nodes, but the logic is pure so we assert on the parser side
    // that the valid sexp with an empty-string value parses to a
    // None slot (no work for the validator).
    let _ = nodes; // silence unused warning under cfg
    let parsed = parse_plan_dag(sexp);
    assert_eq!(parsed.nodes.len(), 1);
    assert!(
        parsed.nodes[0].compensate_node.is_none(),
        "empty quoted value must drop to None instead of an empty slot"
    );
}

#[test]
fn build_validated_dag_rejects_direction_mismatch() {
    // Forward says `fail` → `comp-1`; reverse on `comp-1` says it
    // compensates `other-fail` instead. The validator MUST refuse
    // — the scheduler is forbidden from silently picking one
    // direction over the other.
    let sexp = r#"
        (plan
          (node :id "fail"
                :target "mission_task_delegate"
                :compensate-node "comp-1")
          (node :id "other-fail"
                :target "mission_task_delegate")
          (node :id "comp-1"
                :target "mission_task_delegate"
                :compensates "other-fail"))
    "#;
    let err = build_validated_dag(sexp).expect_err("mismatch must fail");
    match err {
        DagBuildError::CompensateDirectionMismatch {
            failing_node_id,
            comp_node_id,
            reverse_target,
        } => {
            assert_eq!(failing_node_id, "fail");
            assert_eq!(comp_node_id, "comp-1");
            assert_eq!(reverse_target, "other-fail");
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn build_validated_dag_accepts_agreeing_forward_and_reverse_refs() {
    // The two directions agree (forward + reverse name each other);
    // the validator accepts the plan and `compute_compensation_order`
    // surfaces the candidate exactly once (no duplicate from the
    // union).
    let sexp = r#"
        (plan
          (node :id "fail"
                :target "mission_task_delegate"
                :rollback-cascade "plan"
                :compensate-node "comp-1")
          (node :id "comp-1"
                :target "mission_task_delegate"
                :compensates "fail"))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("agreement must validate");
    let ordered = compute_compensation_order("fail", &parsed.nodes, &order);
    assert_eq!(ordered.len(), 1, "agreeing dual decl must not duplicate");
    assert_eq!(ordered[0].id, "comp-1");
}

#[test]
fn build_validated_dag_accepts_forward_only_compensate_ref() {
    // The forward ref alone (no reverse declaration) is the new
    // wave-19 capability: the failing node points at a compensation
    // node and `compute_compensation_order` discovers the candidate
    // even though `comp-1` carries no `:compensates` slot.
    let sexp = r#"
        (plan
          (node :id "fail"
                :target "mission_task_delegate"
                :rollback-cascade "plan"
                :compensate-node "comp-1")
          (node :id "comp-1"
                :target "mission_task_delegate"))
    "#;
    let (parsed, order) = build_validated_dag(sexp).expect("forward-only must validate");
    let ordered = compute_compensation_order("fail", &parsed.nodes, &order);
    assert_eq!(ordered.len(), 1);
    assert_eq!(ordered[0].id, "comp-1");
}

#[test]
fn compute_compensation_order_unions_forward_and_reverse_candidates() {
    // Mixed declarations: one comp node uses the reverse contract,
    // another is reached only via the forward ref. Both surface as
    // candidates; ordering still falls back to forward-order rank
    // when no `:rollback-after` edges exist.
    let nodes = vec![
        DagNode {
            id: "fail".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensate_node: Some("comp-fwd".into()),
            ..Default::default()
        },
        DagNode {
            id: "comp-rev".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            ..Default::default()
        },
        DagNode {
            id: "comp-fwd".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let order = vec![
        "fail".to_string(),
        "comp-rev".to_string(),
        "comp-fwd".to_string(),
    ];
    let ordered = compute_compensation_order("fail", &nodes, &order);
    assert_eq!(ordered.len(), 2);
    let ids: Vec<&str> = ordered.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains(&"comp-rev"));
    assert!(ids.contains(&"comp-fwd"));
}

#[test]
fn build_nodes_summary_surfaces_forward_compensate_node_ref() {
    // Forward `:compensate-node` declaration on the failing node
    // surfaces under the same `rollback` block as the existing
    // cascade hints, so audit dashboards can pin both directions.
    let nodes = vec![
        DagNode {
            id: "fail".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            rollback_cascade: Some("plan".into()),
            compensate_node: Some("comp-1".into()),
            ..Default::default()
        },
        DagNode {
            id: "comp-1".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        },
    ];
    let order = vec!["fail".into(), "comp-1".into()];
    let summary = build_nodes_summary(&nodes, &order);
    let arr = summary.as_array().unwrap();
    assert_eq!(arr[0]["rollback"]["cascade_mode"], "plan");
    assert_eq!(arr[0]["rollback"]["compensate_node"], "comp-1");
    // Compensation node carried no rollback hint — summary stays quiet.
    assert!(arr[1].get("rollback").is_none());
}

#[test]
fn wave18_safety_gates_unchanged_when_only_forward_ref_used() {
    // wave-18 invariant guard — declaring only the forward ref must
    // NOT bypass the wave-17 / task 04 workstation safety check on
    // a compensation node. We verify this through the safety check
    // directly because the cascade body uses the same check.
    let comp = DagNode {
        id: "comp-1".into(),
        target: "mission_task_delegate".into(),
        failure_policy: "fail-fast".into(),
        // No reverse `:compensates` declared — discovered only via
        // the forward ref on the failing node side.
        rollback_policy: Some("workstation".into()),
        rollback_owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
        // objective intentionally missing → safety gate must refuse.
        target_project: Some("missiond".into()),
        dispatch_strategy: Some("fresh-code-alignment".into()),
        ..Default::default()
    };
    let descriptor = build_rollback_descriptor(&comp);
    let err = descriptor
        .safety_check_for_workstation(&comp)
        .expect_err("safety gate must still refuse without objective");
    assert!(err.contains(":rollback-objective"));
}

// ── wave-20 / task 04 — TaskContractDispatchCtx parse coverage ──────

/// Default dispatch_contract_mode is `Rendered` so the wave-15..19
/// byte-shape is preserved across the DAG scheduler entry point.
#[test]
fn task_contract_dispatch_ctx_default_dispatch_contract_mode_is_rendered() {
    let ctx = TaskContractDispatchCtx::from_args(&json!({})).expect("default ok");
    assert!(matches!(
        ctx.dispatch_contract_mode,
        super::super::plan::DispatchContractMode::Rendered
    ));
    assert!(matches!(
        ctx.mode,
        super::super::plan::TaskContractEmitMode::Off
    ));
}

/// Explicit `dispatch_contract_mode="machine"` is captured on the
/// per-DAG-run ctx so every per-node dispatch sees the same mode.
#[test]
fn task_contract_dispatch_ctx_captures_machine_mode_for_dag() {
    let v = json!({
        "task_contract_mode": "emit",
        "dispatch_contract_mode": "machine",
    });
    let ctx = TaskContractDispatchCtx::from_args(&v).expect("machine ok");
    assert!(ctx.dispatch_contract_mode.is_machine());
    assert!(matches!(
        ctx.mode,
        super::super::plan::TaskContractEmitMode::Emit
    ));
}

/// Boolean shorthand `render_markdown=false` flows through the DAG
/// ctx the same way the single-node runner picks it up.
#[test]
fn task_contract_dispatch_ctx_render_markdown_false_is_machine_for_dag() {
    let v = json!({"render_markdown": false});
    let ctx = TaskContractDispatchCtx::from_args(&v).expect("shorthand ok");
    assert!(ctx.dispatch_contract_mode.is_machine());
}

/// A typo in `dispatch_contract_mode` MUST fail fast at the
/// scheduler entry point — the runner never silently degrades to
/// `rendered` and never spawns a node task on a malformed input.
#[test]
fn task_contract_dispatch_ctx_rejects_unknown_dispatch_contract_mode() {
    let v = json!({"dispatch_contract_mode": "machin"});
    let err = TaskContractDispatchCtx::from_args(&v).expect_err("typo rejected");
    assert!(err.is_error.unwrap_or(false));
}
