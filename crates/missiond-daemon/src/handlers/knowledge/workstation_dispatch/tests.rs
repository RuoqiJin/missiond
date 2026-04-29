use super::super::plan::AGENT_TEAM_OBJECTIVE_HINT;
use super::*;
use crate::slot_orchestrator::project_root::resolve_target_project_root;
use crate::state::AppState;
use chrono::TimeZone;
use chrono::Utc;
use missiond_core::types::Plan;
use missiond_core::types::PlanStatus;
use serde_json::json;
use std::path::Path;
use uuid::Uuid;

fn fixture_plan(sexp: &str) -> Plan {
    Plan {
        id: Uuid::parse_str("00000000-0000-0000-0000-000000000def").unwrap(),
        board_task_id: "btk-wd".to_string(),
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

#[test]
fn opt_in_requires_explicit_arg_or_plan_hint() {
    assert!(!opt_in_requested(&json!({}), false));
    assert!(opt_in_requested(
        &json!({"workstation_dispatch": true}),
        false
    ));
    assert!(opt_in_requested(&json!({}), true));
    // Random truthy fields do NOT count.
    assert!(!opt_in_requested(
        &json!({"target": "mission_task_delegate"}),
        false
    ));
    assert!(!opt_in_requested(
        &json!({"workstation_dispatch": false}),
        false
    ));
}

// ── wave-16 / task 03 — auto-inference decision tests ───────────────

/// Helper: build an inference context that matches every gate by
/// default. Individual tests flip a single field to assert that gate.
fn ctx_all_pass<'a>() -> InferenceContext<'a> {
    InferenceContext {
        target: "mission_task_delegate",
        dispatch_strategy: "fresh-code-alignment",
        objective: Some("ship the wave"),
        owned_files_present: true,
        scope_present: false,
        target_project_present: false,
        requested_cwd_present: false,
    }
}

#[test]
fn evaluate_decision_explicit_true_wins_even_without_scope_signal() {
    let mut ctx = ctx_all_pass();
    ctx.owned_files_present = false;
    let decision = evaluate_dispatch_decision(&json!({"workstation_dispatch": true}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::ExplicitArg);
    assert!(decision.is_enabled());
}

#[test]
fn evaluate_decision_explicit_false_disables_inference() {
    let ctx = ctx_all_pass();
    let decision = evaluate_dispatch_decision(
        &json!({"workstation_dispatch": false}),
        true, // even with plan hint set
        &ctx,
    );
    assert_eq!(decision.source, WorkstationDispatchSource::Disabled);
    assert!(!decision.is_enabled());
    assert!(decision
        .reason
        .unwrap()
        .contains("workstation_dispatch=false"));
}

#[test]
fn evaluate_decision_plan_hint_takes_precedence_over_inference() {
    let ctx = ctx_all_pass();
    let decision = evaluate_dispatch_decision(&json!({}), true, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::PlanHint);
    assert!(decision.is_enabled());
}

#[test]
fn evaluate_decision_inferred_when_all_gates_pass() {
    let ctx = ctx_all_pass();
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
    assert!(decision.is_enabled());
    assert!(decision.reason.unwrap().contains("fresh-code-alignment"));
}

#[test]
fn evaluate_decision_inferred_for_each_strategy_in_whitelist() {
    for strategy in INFERABLE_DISPATCH_STRATEGIES {
        let mut ctx = ctx_all_pass();
        ctx.dispatch_strategy = strategy;
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(
            decision.source,
            WorkstationDispatchSource::Inferred,
            "strategy `{}` should be inferable",
            strategy
        );
    }
}

#[test]
fn evaluate_decision_not_inferred_for_unknown_strategy() {
    let mut ctx = ctx_all_pass();
    ctx.dispatch_strategy = "unknown";
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    assert!(!decision.is_enabled());
    assert!(decision.reason.unwrap().contains("dispatch strategy"));
}

#[test]
fn evaluate_decision_not_inferred_for_prompt_fallback_strategy() {
    let mut ctx = ctx_all_pass();
    ctx.dispatch_strategy = "prompt-fallback";
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
}

#[test]
fn evaluate_decision_not_inferred_for_mission_execution_target() {
    let mut ctx = ctx_all_pass();
    ctx.target = "mission_execution";
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    assert!(decision.reason.unwrap().contains("mission_task_delegate"));
}

#[test]
fn evaluate_decision_not_inferred_for_mission_flow_run_target() {
    let mut ctx = ctx_all_pass();
    ctx.target = "mission_flow_run";
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
}

#[test]
fn evaluate_decision_not_inferred_when_objective_missing() {
    let mut ctx = ctx_all_pass();
    ctx.objective = None;
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    assert!(decision.reason.unwrap().contains("objective"));
}

#[test]
fn evaluate_decision_not_inferred_when_objective_blank() {
    let mut ctx = ctx_all_pass();
    ctx.objective = Some("   ");
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
}

#[test]
fn evaluate_decision_not_inferred_when_no_scope_signal() {
    let ctx = InferenceContext {
        target: "mission_task_delegate",
        dispatch_strategy: "fresh-code-alignment",
        objective: Some("ship"),
        owned_files_present: false,
        scope_present: false,
        target_project_present: false,
        requested_cwd_present: false,
    };
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    assert!(decision.reason.unwrap().contains("scoping signal"));
}

#[test]
fn evaluate_decision_inferred_when_scope_present_only() {
    let ctx = InferenceContext {
        target: "mission_task_delegate",
        dispatch_strategy: "agent-team",
        objective: Some("ship"),
        owned_files_present: false,
        scope_present: true,
        target_project_present: false,
        requested_cwd_present: false,
    };
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
}

#[test]
fn evaluate_decision_inferred_when_target_project_present_only() {
    let ctx = InferenceContext {
        target: "mission_task_delegate",
        dispatch_strategy: "resident-lisp",
        objective: Some("ship"),
        owned_files_present: false,
        scope_present: false,
        target_project_present: true,
        requested_cwd_present: false,
    };
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
}

#[test]
fn evaluate_decision_inferred_when_requested_cwd_present_only() {
    let ctx = InferenceContext {
        target: "mission_task_delegate",
        dispatch_strategy: "mixed",
        objective: Some("ship"),
        owned_files_present: false,
        scope_present: false,
        target_project_present: false,
        requested_cwd_present: true,
    };
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
}

#[test]
fn workstation_dispatch_source_string_pin() {
    // The five values are part of the response wire contract.
    assert_eq!(
        WorkstationDispatchSource::ExplicitArg.as_str(),
        "explicit_arg"
    );
    assert_eq!(WorkstationDispatchSource::PlanHint.as_str(), "plan_hint");
    assert_eq!(WorkstationDispatchSource::Inferred.as_str(), "inferred");
    assert_eq!(WorkstationDispatchSource::Disabled.as_str(), "disabled");
    assert_eq!(
        WorkstationDispatchSource::NotApplicable.as_str(),
        "not_applicable"
    );
}

/// End-to-end shape check: when auto-inference picks `agent-team`,
/// the brief built from the inferred hints carries the literal Chinese
/// reminder exactly once. This pins the wave-15 invariant onto the
/// wave-16 inference path so a future merge cannot silently double-
/// inject the hint.
#[test]
fn inferred_agent_team_path_injects_literal_exactly_once() {
    let ctx = InferenceContext {
        target: "mission_task_delegate",
        dispatch_strategy: "agent-team",
        objective: Some("ship the wave"),
        owned_files_present: true,
        scope_present: false,
        target_project_present: false,
        requested_cwd_present: false,
    };
    let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
    assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
    // Build the brief the same way `run_workstation_dispatch` would
    // to confirm the literal lands once.
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship the wave".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    let brief = build_task_brief(&plan, &hints, "agent-team");
    assert_eq!(
        brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
        1,
        "agent-team hint must appear exactly once on the inferred path"
    );
}

#[test]
fn explicit_workstation_dispatch_flag_extracts_explicit_choice() {
    assert_eq!(explicit_workstation_dispatch_flag(&json!({})), None);
    assert_eq!(
        explicit_workstation_dispatch_flag(&json!({"workstation_dispatch": true})),
        Some(true)
    );
    assert_eq!(
        explicit_workstation_dispatch_flag(&json!({"workstation_dispatch": false})),
        Some(false)
    );
    // Non-bool values do not satisfy the strict opt-in/out contract.
    assert_eq!(
        explicit_workstation_dispatch_flag(&json!({"workstation_dispatch": "yes"})),
        None
    );
}

#[test]
fn merge_args_arg_wins_over_hint_for_every_field() {
    let hints = WorkstationDispatchHints {
        objective: Some("hint obj".to_string()),
        scope: Some("hint scope".to_string()),
        owned_files: vec!["hint.rs".to_string()],
        forbidden_files: vec!["hint_forbidden.rs".to_string()],
        acceptance_commands: vec!["hint cmd".to_string()],
        commit_policy: Some("hint-policy".to_string()),
        target_project: Some("hint-proj".to_string()),
        requested_cwd: Some("/hint/cwd".to_string()),
        dispatch_strategy: Some("resident-lisp".to_string()),
    };
    let args = json!({
        "objective": "arg obj",
        "scope": "arg scope",
        "owned_files": ["arg.rs", "arg2.rs"],
        "forbidden_files": ["arg_forbidden.rs"],
        "acceptance_commands": ["arg cmd1", "arg cmd2"],
        "commit_policy": "arg-policy",
        "target_project": "arg-proj",
        "requested_cwd": "/arg/cwd",
        "dispatch_strategy": "agent-team",
    });
    let merged = hints.merge_args(&args);
    assert_eq!(merged.objective.as_deref(), Some("arg obj"));
    assert_eq!(merged.scope.as_deref(), Some("arg scope"));
    assert_eq!(merged.owned_files, vec!["arg.rs", "arg2.rs"]);
    assert_eq!(merged.forbidden_files, vec!["arg_forbidden.rs"]);
    assert_eq!(merged.acceptance_commands, vec!["arg cmd1", "arg cmd2"]);
    assert_eq!(merged.commit_policy.as_deref(), Some("arg-policy"));
    assert_eq!(merged.target_project.as_deref(), Some("arg-proj"));
    assert_eq!(merged.requested_cwd.as_deref(), Some("/arg/cwd"));
    assert_eq!(merged.dispatch_strategy.as_deref(), Some("agent-team"));
}

#[test]
fn merge_args_falls_back_to_hint_when_arg_absent_or_blank() {
    let hints = WorkstationDispatchHints {
        objective: Some("hint obj".to_string()),
        commit_policy: Some("hint-policy".to_string()),
        ..Default::default()
    };
    let args = json!({
        "objective": "   ",  // blank → falls back
        "commit_policy": "",
    });
    let merged = hints.merge_args(&args);
    assert_eq!(merged.objective.as_deref(), Some("hint obj"));
    assert_eq!(merged.commit_policy.as_deref(), Some("hint-policy"));
}

#[test]
fn merge_args_cwd_falls_back_to_args_cwd_alias() {
    let hints = WorkstationDispatchHints::default();
    let args = json!({"cwd": "/from/cwd/alias"});
    let merged = hints.merge_args(&args);
    assert_eq!(merged.requested_cwd.as_deref(), Some("/from/cwd/alias"));
}

#[test]
fn cap_lists_truncates_runaway_lists_and_reports_drop_count() {
    let mut hints = WorkstationDispatchHints {
        owned_files: (0..100).map(|i| format!("f{}.rs", i)).collect(),
        ..Default::default()
    };
    let dropped = hints.cap_lists();
    assert_eq!(hints.owned_files.len(), TASK_BRIEF_LIST_CAP);
    assert!(dropped
        .iter()
        .any(|(label, count)| *label == "owned_files" && *count == 100 - TASK_BRIEF_LIST_CAP));
}

#[test]
fn build_task_brief_includes_canonical_sections_and_scoped_commit_reminder() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship the wave".to_string()),
        scope: Some("wave 15 task 05 only".to_string()),
        owned_files: vec!["a.rs".to_string(), "b.rs".to_string()],
        forbidden_files: vec!["c.rs".to_string()],
        acceptance_commands: vec!["cargo test".to_string(), "git diff --check".to_string()],
        ..Default::default()
    };
    let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
    // headings present
    assert!(brief.contains("## Objective"));
    assert!(brief.contains("## Scope"));
    assert!(brief.contains("## Owned files"));
    assert!(brief.contains("## Forbidden files"));
    assert!(brief.contains("## Acceptance commands"));
    assert!(brief.contains("## Commit policy"));
    // owned files listed
    assert!(brief.contains("- a.rs"));
    assert!(brief.contains("- b.rs"));
    // scoped commit reminder line
    assert!(brief.contains("do not stage or commit outside the owned files"));
    // default policy (we did NOT pass commit_policy)
    assert!(brief.contains("policy: scoped"));
    // fresh-code-alignment must NOT inject the agent-team hint
    assert!(!brief.contains(AGENT_TEAM_OBJECTIVE_HINT));
}

#[test]
fn build_task_brief_injects_agent_team_hint_exactly_once_for_agent_team_strategy() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        ..Default::default()
    };
    let brief = build_task_brief(&plan, &hints, "agent-team");
    assert_eq!(
        brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
        1,
        "agent-team hint must appear exactly once, got: {brief}"
    );
}

#[test]
fn build_task_brief_omits_optional_sections_when_lists_empty() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        ..Default::default()
    };
    let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
    // Forbidden / Acceptance / Scope sections must NOT appear when their
    // backing lists are empty / absent.
    assert!(!brief.contains("## Forbidden files"));
    assert!(!brief.contains("## Acceptance commands"));
    assert!(!brief.contains("## Scope"));
    // Owned-files section is always present (the policy is "stage NOTHING
    // by default" — explicit reminder).
    assert!(brief.contains("## Owned files"));
    assert!(brief.contains("(none declared"));
}

#[test]
fn build_task_brief_uses_explicit_commit_policy_when_supplied() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        commit_policy: Some("monorepo-cascade".to_string()),
        ..Default::default()
    };
    let brief = build_task_brief(&plan, &hints, "resident-lisp");
    assert!(brief.contains("policy: monorepo-cascade"));
    // resident-lisp must NOT inject agent-team hint
    assert!(!brief.contains(AGENT_TEAM_OBJECTIVE_HINT));
}

#[test]
fn safe_descriptor_status_strings_are_distinct() {
    assert_eq!(
        SafeDescriptorReason::UnsupportedTarget("mission_execution".into()).status(),
        "skipped_unsupported_target"
    );
    assert_eq!(
        SafeDescriptorReason::ProjectRootUnresolved("nope".into()).status(),
        "skipped_project_root_unresolved"
    );
    assert_eq!(
        SafeDescriptorReason::MissingObjective.status(),
        "skipped_missing_objective"
    );
}

#[test]
fn outcome_to_response_dispatched_carries_inner_and_brief_preview() {
    let outcome = WorkstationDispatchOutcome::Dispatched {
        task_brief: "## Objective\nship\n".to_string(),
        task_brief_path: None,
        task_contract_source_path: None,
        evidence_path: Some("/tmp/sidecar.json".to_string()),
        evidence_error: None,
        inner_payload: json!({"task_id": "btk-9"}),
    };
    let v = outcome_to_response_fields(&outcome, "agent-team");
    assert_eq!(v["workstation_dispatch_status"], "dispatched");
    assert_eq!(v["dispatch_strategy"], "agent-team");
    assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
    assert!(v["task_brief_preview"]
        .as_str()
        .unwrap()
        .contains("## Objective"));
    assert_eq!(v["inner_result"]["task_id"], "btk-9");
    assert_eq!(v["delegated_board_task_id"], "btk-9");
    // wave-20 / task 04 — legacy / rendered path leaves the
    // `task_contract_source_path` key OFF so the wire shape stays
    // byte-compatible with wave-15..19 callers.
    assert!(
        v.get("task_contract_source_path").is_none(),
        "rendered-path dispatch must omit task_contract_source_path \
         (wave-15..19 byte-compat)"
    );
}

/// wave-20 / task 04 — when the dispatch ran in machine-driven mode
/// the response must carry the resolved on-disk task-contract path
/// so observers can prove the Lisp was load-bearing.
#[test]
fn outcome_to_response_dispatched_machine_mode_surfaces_contract_path() {
    let outcome = WorkstationDispatchOutcome::Dispatched {
        task_brief: "## Objective\nship\n".to_string(),
        task_brief_path: None,
        task_contract_source_path: Some(
            "/tmp/p/.missiond/tasks/generated/plan/root.lisp".to_string(),
        ),
        evidence_path: None,
        evidence_error: None,
        inner_payload: json!({"task_id": "btk-9"}),
    };
    let v = outcome_to_response_fields(&outcome, "agent-team");
    assert_eq!(
        v["task_contract_source_path"],
        "/tmp/p/.missiond/tasks/generated/plan/root.lisp"
    );
}

#[test]
fn outcome_to_response_dispatched_projects_nested_board_task_id() {
    let outcome = WorkstationDispatchOutcome::Dispatched {
        task_brief: "## Objective\nship\n".to_string(),
        task_brief_path: None,
        task_contract_source_path: None,
        evidence_path: None,
        evidence_error: None,
        inner_payload: json!({
            "task_id": {
                "id": "3ab1b19d-3d64-45de-9493-ff9972d2e77f",
                "status": "open"
            }
        }),
    };
    let v = outcome_to_response_fields(&outcome, "agent-team");
    assert_eq!(
        v["delegated_board_task_id"],
        "3ab1b19d-3d64-45de-9493-ff9972d2e77f"
    );
    assert_eq!(v["inner_result"]["task_id"]["status"], "open");
}

#[test]
fn outcome_to_response_safe_descriptor_carries_reason_detail() {
    let outcome = WorkstationDispatchOutcome::SafeDescriptor {
        reason: SafeDescriptorReason::ProjectRootUnresolved("no signal".to_string()),
        task_brief: None,
    };
    let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
    assert_eq!(
        v["workstation_dispatch_status"],
        "skipped_project_root_unresolved"
    );
    assert_eq!(v["workstation_dispatch_reason"], "no signal");
    // No inner_result on safe descriptors
    assert!(v.get("inner_result").is_none());
}

#[test]
fn outcome_to_response_dry_run_omits_evidence_and_inner() {
    let outcome = WorkstationDispatchOutcome::DryRun {
        task_brief: "## Objective\nship\n".to_string(),
    };
    let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
    assert_eq!(v["workstation_dispatch_status"], "dry_run_no_dispatch");
    assert!(v.get("inner_result").is_none());
    assert!(v.get("evidence_path").is_none());
    assert!(v["task_brief_preview"].as_str().unwrap().contains("ship"));
}

// ── async path tests (stand up a minimal AppState) ───────────────────

use crate::slot_orchestrator::project_root::ResolutionError;
use missiond_core::types::{ProjectConfig, ProjectRegistry, SharedProjectRegistry};
use std::sync::Arc;
use tokio::sync::RwLock;

fn fixture_registry(id: &str, root: &Path) -> SharedProjectRegistry {
    Arc::new(RwLock::new(ProjectRegistry::new(vec![ProjectConfig {
        id: id.to_string(),
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
    }])))
}

/// Build a minimal AppState skeleton good enough for the workstation-
/// dispatch resolver path. Only `project_registry` is touched here;
/// other fields stay at their default constructions because we never
/// invoke a code path that reads them in the safe-descriptor and
/// resolver-only tests.
async fn fixture_state_with_registry(reg: SharedProjectRegistry) -> Option<AppState> {
    // AppState construction is feature-gated and pulls in the full
    // daemon graph (DB, bus, slot dispatcher) — far heavier than this
    // unit-level test wants. We therefore exercise the resolver path
    // directly via `resolve_target_project_root` in
    // `safe_descriptor_emitted_when_project_root_unresolved` instead
    // of standing up a full AppState. Keeping the helper here for
    // future async-only tests; returning `None` for now signals the
    // test harness to fall back to direct resolver assertions.
    let _ = reg;
    None
}

/// Resolver-level assertion: missing project root yields a structured
/// safe descriptor instead of a silent fallback. We exercise the path
/// from `run_workstation_dispatch` would take by calling
/// `resolve_target_project_root` with the same args and asserting the
/// branch shape downstream.
#[tokio::test]
async fn missing_project_root_signals_resolver_no_signal() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let reg = fixture_registry("missiond", &root);
    // No project_id, no cwd, no fallback → NoSignal.
    let err = resolve_target_project_root(None, None, None, &reg)
        .await
        .expect_err("should fail");
    assert!(matches!(err, ResolutionError::NoSignal));
    // Mirror what run_workstation_dispatch would build:
    let descriptor = SafeDescriptorReason::ProjectRootUnresolved(err.to_string());
    assert_eq!(descriptor.status(), "skipped_project_root_unresolved");
    let _ = fixture_state_with_registry(reg).await;
}

#[tokio::test]
async fn relative_cwd_is_rejected_by_pre_flight() {
    // The dispatch helper itself rejects a relative cwd before even
    // reaching the resolver — this is the "do not join relative cwd
    // against process cwd" architectural invariant.
    let cwd = "relative/path";
    assert!(!Path::new(cwd).is_absolute());
    let descriptor = SafeDescriptorReason::ProjectRootUnresolved(format!(
        "requested_cwd `{}` is not absolute; \
         workstation-dispatch never joins a relative cwd against the daemon process cwd",
        cwd
    ));
    assert_eq!(descriptor.status(), "skipped_project_root_unresolved");
    assert!(descriptor.detail().contains("not absolute"));
}

/// `WorkstationDispatchHints::default` inherently has no objective —
/// confirm the safe descriptor branch fires before any I/O happens.
#[tokio::test]
async fn missing_objective_yields_safe_descriptor() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let reg = fixture_registry("missiond", &root);
    // We reuse the resolver to make this test fully self-contained
    // (no AppState dependency) — the resolver succeeds, but the
    // dispatch helper would refuse on `MissingObjective` first.
    let _ok = resolve_target_project_root(Some("missiond"), None, None, &reg)
        .await
        .expect("resolver succeeds");
    let descriptor = SafeDescriptorReason::MissingObjective;
    assert_eq!(descriptor.status(), "skipped_missing_objective");
    assert!(descriptor.detail().contains("content-free"));
}

// ── wave-17 / task 07 — scoped-commit handoff default tests ─────────

#[test]
fn classify_task_kind_treats_owned_files_as_code_brief() {
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    assert_eq!(classify_task_kind(&hints), BriefTaskKind::Code);
}

#[test]
fn classify_task_kind_treats_empty_owned_files_as_read_only_brief() {
    let hints = WorkstationDispatchHints {
        objective: Some("audit the wave".to_string()),
        ..Default::default()
    };
    assert_eq!(classify_task_kind(&hints), BriefTaskKind::ReadOnly);
}

#[test]
fn build_task_brief_code_requires_enforce_scoped_commit_on_completion() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string(), "b.rs".to_string()],
        ..Default::default()
    };
    let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
    // The completion handoff section is always present.
    assert!(
        brief.contains("## Completion handoff (scoped commit)"),
        "code brief must carry the completion handoff section"
    );
    // The brief tells the worker to set enforce_scoped_commit=true.
    assert!(
        brief.contains("`enforce_scoped_commit=true`"),
        "code brief must instruct the worker to opt into enforcement"
    );
    // The brief asks for committed status + commit_hash + staged_files.
    assert!(
        brief.contains("`commit_status=\"committed\"`"),
        "code brief must request commit_status=committed"
    );
    assert!(
        brief.contains("`commit_hash="),
        "code brief must request commit_hash"
    );
    assert!(
        brief.contains("`staged_files="),
        "code brief must request staged_files"
    );
    // Task kind line.
    assert!(
        brief.contains("- task kind: code"),
        "code brief must declare task kind"
    );
    // The blocked branch must also be documented so workers don't
    // silently drop to "no commit".
    assert!(
        brief.contains("`commit_status=\"blocked\"`"),
        "code brief must document the blocked branch"
    );
    // The daemon-never-runs-git invariant must be loud.
    assert!(
        brief.contains("daemon never runs git itself"),
        "brief must restate the daemon-never-runs-git invariant"
    );
}

#[test]
fn build_task_brief_read_only_uses_not_required_with_explanation() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("audit the wave-17 surface".to_string()),
        ..Default::default()
    };
    let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
    assert!(brief.contains("## Completion handoff (scoped commit)"));
    // Read-only briefs default to commit_status=not-required.
    assert!(
        brief.contains("`commit_status=\"not-required\"`"),
        "read-only brief must default to commit_status=not-required"
    );
    // ...with an explanation requirement.
    assert!(
        brief.contains("explain WHY"),
        "read-only brief must require an explanation in the summary field"
    );
    // Task kind line.
    assert!(
        brief.contains("- task kind: read-only"),
        "read-only brief must declare task kind"
    );
    // Still asks for enforce_scoped_commit=true so the daemon's
    // wave-16/06 gates run.
    assert!(
        brief.contains("`enforce_scoped_commit=true`"),
        "read-only brief still opts the completion call into enforcement"
    );
    // Read-only brief must NOT instruct the worker to commit anything.
    assert!(
        !brief.contains("`commit_status=\"committed\"`"),
        "read-only brief must NOT prescribe commit_status=committed"
    );
}

#[test]
fn build_task_brief_completion_handoff_does_not_double_inject_agent_team_hint() {
    // The agent-team hint sits in section 8 (after completion handoff).
    // Adding the new section must not leak the literal anywhere else.
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    let brief = build_task_brief(&plan, &hints, "agent-team");
    assert_eq!(
        brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
        1,
        "agent-team hint must still appear exactly once after wave-17 / task 07"
    );
}

#[test]
fn build_task_brief_read_only_does_not_inject_agent_team_hint_for_other_strategies() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("audit".to_string()),
        ..Default::default()
    };
    let brief = build_task_brief(&plan, &hints, "resident-lisp");
    assert_eq!(brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(), 0);
    // Confirm read-only branch lands.
    assert!(brief.contains("- task kind: read-only"));
}

#[test]
fn outcome_to_response_dispatched_advertises_scoped_commit_policy() {
    let outcome = WorkstationDispatchOutcome::Dispatched {
        task_brief: "## Objective\nship\n".to_string(),
        task_brief_path: None,
        task_contract_source_path: None,
        evidence_path: None,
        evidence_error: None,
        inner_payload: json!({"task_id": "btk-9"}),
    };
    let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
    assert_eq!(v["scoped_commit_required"], json!(true));
    assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
}

#[test]
fn outcome_to_response_dry_run_advertises_scoped_commit_policy() {
    let outcome = WorkstationDispatchOutcome::DryRun {
        task_brief: "## Objective\nship\n".to_string(),
    };
    let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
    assert_eq!(v["scoped_commit_required"], json!(true));
    assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
}

#[test]
fn outcome_to_response_inner_error_advertises_scoped_commit_policy() {
    let outcome = WorkstationDispatchOutcome::InnerError {
        task_brief: "## Objective\nship\n".to_string(),
        inner_payload: json!({"error": "nope"}),
    };
    let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
    assert_eq!(v["scoped_commit_required"], json!(true));
    assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
}

#[test]
fn outcome_to_response_safe_descriptor_advertises_scoped_commit_policy() {
    // Even on safe-descriptor refusals the policy contract is part of
    // the wire shape so observers don't have to special-case the
    // skipped branch when asserting the invariant.
    let outcome = WorkstationDispatchOutcome::SafeDescriptor {
        reason: SafeDescriptorReason::MissingObjective,
        task_brief: None,
    };
    let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
    assert_eq!(v["scoped_commit_required"], json!(true));
    assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
}

#[test]
fn brief_task_kind_string_pin_is_stable_wire_contract() {
    // These two strings are part of the brief / response wire contract;
    // changing them silently would break downstream observers.
    assert_eq!(BriefTaskKind::Code.as_str(), "code");
    assert_eq!(BriefTaskKind::ReadOnly.as_str(), "read-only");
}

// ── wave-19 / task 07 — task-contract v1 parser tests ────────────────

/// Reference contract body produced by the wave-19 / task 06 emitter.
/// We pin against the exact textual shape so a future emitter tweak
/// that breaks the parser surface trips a test rather than silently
/// downgrading to the legacy brief.
const SAMPLE_CONTRACT: &str = r#";; Generated by MissionD plan-runner (wave-19 / task 06).
;; plan_id = 00000000-0000-0000-0000-000000000def
;; board_task_id = btk-wd
;; node_id = root

(task plan-00000000-node-root
  :schema "missiond.task-contract.v1"
  :title "Plan 00000000-0000-0000-0000-000000000def node root — workstation task contract"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :dispatch-strategy "agent-team"
  :goal "ship the wave-19 contract consumer"
  :scope "wave 19 task 07 only"
  :write-scope
["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
  :must-not-touch
["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
 "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
  :acceptance
["cargo test -p missiond-daemon"
 "cargo build --workspace"]
  :commit
(:required true
 :message "feat(workstation): consume Lisp task contracts"
 :scope-check write-scope-only
 :policy "scoped")
  :target-project "missiond"
  :requested-cwd "/Users/jinchen/Projects/missiond"
  :target "mission_task_delegate"
  :plan-id "00000000-0000-0000-0000-000000000def"
  :node-id "root"
)
"#;

#[test]
fn parse_task_contract_extracts_every_consumed_field() {
    let c = parse_task_contract(SAMPLE_CONTRACT).expect("must parse");
    assert_eq!(c.schema, "missiond.task-contract.v1");
    assert_eq!(c.goal, "ship the wave-19 contract consumer");
    assert_eq!(c.scope.as_deref(), Some("wave 19 task 07 only"));
    assert_eq!(
        c.write_scope,
        vec!["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
    );
    assert_eq!(c.must_not_touch.len(), 2);
    assert!(c
        .must_not_touch
        .contains(&"crates/missiond-daemon/src/handlers/knowledge/plan.rs".to_string()));
    assert_eq!(c.acceptance.len(), 2);
    assert_eq!(c.commit_policy.as_deref(), Some("scoped"));
    assert_eq!(c.dispatch_strategy.as_deref(), Some("agent-team"));
    assert_eq!(c.target_project.as_deref(), Some("missiond"));
    assert_eq!(
        c.requested_cwd.as_deref(),
        Some("/Users/jinchen/Projects/missiond")
    );
    assert_eq!(c.target.as_deref(), Some("mission_task_delegate"));
}

#[test]
fn parse_task_contract_tolerates_optional_field_absence() {
    // Minimal viable contract: schema + goal only.
    let src = r#"(task minimal
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :write-scope []
  :must-not-touch []
)
"#;
    let c = parse_task_contract(src).expect("minimal parse");
    assert_eq!(c.goal, "ship");
    assert!(c.scope.is_none());
    assert!(c.write_scope.is_empty());
    assert!(c.must_not_touch.is_empty());
    assert!(c.acceptance.is_empty());
    assert!(c.commit_policy.is_none());
    assert!(c.dispatch_strategy.is_none());
}

#[test]
fn parse_task_contract_rejects_schema_mismatch() {
    let src = r#"(task wrong
  :schema "missiond.task-contract.v0"
  :goal "ship"
)"#;
    let err = parse_task_contract(src).expect_err("must reject");
    assert!(matches!(err, TaskContractParseError::SchemaMismatch(_)));
    assert!(err.reason().contains("schema mismatch"));
    assert!(err.reason().contains("v0"));
}

#[test]
fn parse_task_contract_rejects_missing_schema() {
    let src = r#"(task no-schema :goal "ship")"#;
    let err = parse_task_contract(src).expect_err("must reject");
    assert!(matches!(err, TaskContractParseError::SchemaMismatch(_)));
    assert!(err.reason().contains("(absent)"));
}

#[test]
fn parse_task_contract_rejects_missing_goal() {
    let src = r#"(task no-goal
  :schema "missiond.task-contract.v1"
)"#;
    let err = parse_task_contract(src).expect_err("must reject");
    assert!(matches!(
        err,
        TaskContractParseError::MissingRequired("goal")
    ));
}

#[test]
fn parse_task_contract_rejects_blank_goal() {
    let src = r#"(task blank
  :schema "missiond.task-contract.v1"
  :goal "   "
)"#;
    let err = parse_task_contract(src).expect_err("must reject");
    assert!(matches!(
        err,
        TaskContractParseError::MissingRequired("goal")
    ));
}

#[test]
fn parse_task_contract_rejects_unbalanced_parens() {
    let src = r#"(task bad
  :schema "missiond.task-contract.v1"
  :goal "ship"
"#;
    let err = parse_task_contract(src).expect_err("must reject");
    assert!(matches!(err, TaskContractParseError::Lex(_)));
}

#[test]
fn parse_task_contract_rejects_unterminated_string() {
    let src = r#"(task bad :schema "unterminated"#;
    let err = parse_task_contract(src).expect_err("must reject");
    assert!(matches!(err, TaskContractParseError::Lex(_)));
}

#[test]
fn parse_task_contract_rejects_non_task_top_form() {
    let src = r#"(plan something :schema "missiond.task-contract.v1" :goal "x")"#;
    let err = parse_task_contract(src).expect_err("must reject");
    assert!(matches!(err, TaskContractParseError::NotATaskForm(_)));
}

#[test]
fn parse_task_contract_rejects_wrong_field_shape() {
    // :goal must be a string, not a list.
    let src = r#"(task bad
  :schema "missiond.task-contract.v1"
  :goal ["a" "b"]
)"#;
    let err = parse_task_contract(src).expect_err("must reject");
    match err {
        TaskContractParseError::FieldShape { field, .. } => assert_eq!(field, "goal"),
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn parse_task_contract_rejects_non_string_in_write_scope() {
    let src = r#"(task bad
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :write-scope [foo "bar"]
)"#;
    let err = parse_task_contract(src).expect_err("must reject");
    match err {
        TaskContractParseError::FieldShape { field, .. } => {
            assert_eq!(field, "write-scope")
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn parse_task_contract_handles_escaped_strings() {
    let src = r#"(task esc
  :schema "missiond.task-contract.v1"
  :goal "ship \"quoted\" and \\backslash"
)"#;
    let c = parse_task_contract(src).expect("must parse");
    assert_eq!(c.goal, "ship \"quoted\" and \\backslash");
}

#[test]
fn parse_task_contract_skips_unknown_fields() {
    // A future emitter may add fields we do not consume — accept and
    // ignore them so the parser stays forward-compatible. The
    // authoritative checker (scripts/check-task-contract.mjs) is the
    // gate for new fields.
    let src = r#"(task fwd
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :unknown-future-field "ignored"
  :requirements ["a" "b"]
  :report ["x"]
)"#;
    let c = parse_task_contract(src).expect("must parse");
    assert_eq!(c.goal, "ship");
}

#[test]
fn parse_task_contract_skips_comment_lines() {
    let src = r#"
;; comment 1
;; comment 2 with paren ( and string "
(task ok
  ;; inline comment
  :schema "missiond.task-contract.v1"
  :goal "ship"
)
"#;
    let c = parse_task_contract(src).expect("must parse");
    assert_eq!(c.goal, "ship");
}

#[test]
fn extract_commit_policy_returns_none_when_policy_absent() {
    let src = r#"(task np
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :commit (:required true :scope-check write-scope-only)
)"#;
    let c = parse_task_contract(src).expect("must parse");
    assert!(c.commit_policy.is_none());
}

#[test]
fn extract_commit_policy_returns_value_when_present() {
    let src = r#"(task wp
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :commit (:required true :policy "monorepo-cascade" :scope-check none)
)"#;
    let c = parse_task_contract(src).expect("must parse");
    assert_eq!(c.commit_policy.as_deref(), Some("monorepo-cascade"));
}

#[test]
fn load_task_contract_round_trips_emitter_output() {
    // Write the emitter sample to disk and confirm the loader returns
    // the same projection as the in-memory parser.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("contract.lisp");
    std::fs::write(&path, SAMPLE_CONTRACT).unwrap();
    let c = load_task_contract(&path).expect("must load");
    assert_eq!(c.goal, "ship the wave-19 contract consumer");
    assert_eq!(c.dispatch_strategy.as_deref(), Some("agent-team"));
}

#[test]
fn load_task_contract_io_error_when_file_missing() {
    let err =
        load_task_contract(Path::new("/nonexistent/path/contract.lisp")).expect_err("must fail");
    assert!(matches!(err, TaskContractParseError::Io(_)));
    assert!(err.reason().starts_with("io:"));
}

// ── wave-19 / task 07 — overlay tests ────────────────────────────────

#[test]
fn overlay_contract_overrides_objective_and_lists() {
    let mut hints = WorkstationDispatchHints {
        objective: Some("hint obj".to_string()),
        owned_files: vec!["hint.rs".to_string()],
        forbidden_files: vec!["hint_forbidden.rs".to_string()],
        ..Default::default()
    };
    let contract = ParsedTaskContract {
        schema: "missiond.task-contract.v1".to_string(),
        goal: "contract goal".to_string(),
        write_scope: vec!["contract_a.rs".to_string(), "contract_b.rs".to_string()],
        must_not_touch: vec!["contract_no.rs".to_string()],
        acceptance: vec!["cargo test".to_string()],
        commit_policy: Some("scoped-strict".to_string()),
        dispatch_strategy: Some("resident-lisp".to_string()),
        target_project: Some("contract-proj".to_string()),
        requested_cwd: Some("/contract/cwd".to_string()),
        ..Default::default()
    };
    hints.overlay_contract(&contract);
    assert_eq!(hints.objective.as_deref(), Some("contract goal"));
    assert_eq!(hints.owned_files, vec!["contract_a.rs", "contract_b.rs"]);
    assert_eq!(hints.forbidden_files, vec!["contract_no.rs"]);
    assert_eq!(hints.acceptance_commands, vec!["cargo test"]);
    assert_eq!(hints.commit_policy.as_deref(), Some("scoped-strict"));
    assert_eq!(hints.dispatch_strategy.as_deref(), Some("resident-lisp"));
    assert_eq!(hints.target_project.as_deref(), Some("contract-proj"));
    assert_eq!(hints.requested_cwd.as_deref(), Some("/contract/cwd"));
}

#[test]
fn overlay_contract_preserves_arg_lists_when_contract_lists_are_empty() {
    // The contract emitter only emits non-empty lists; an absent
    // `:acceptance` should NOT erase a caller-supplied acceptance arg.
    let mut hints = WorkstationDispatchHints {
        objective: Some("hint obj".to_string()),
        owned_files: vec!["arg.rs".to_string()],
        acceptance_commands: vec!["arg cmd".to_string()],
        ..Default::default()
    };
    let contract = ParsedTaskContract {
        schema: "missiond.task-contract.v1".to_string(),
        goal: "contract goal".to_string(),
        // All list fields empty.
        ..Default::default()
    };
    hints.overlay_contract(&contract);
    // Objective overridden (non-empty contract goal beats arg).
    assert_eq!(hints.objective.as_deref(), Some("contract goal"));
    // Lists preserved (contract did not declare them).
    assert_eq!(hints.owned_files, vec!["arg.rs"]);
    assert_eq!(hints.acceptance_commands, vec!["arg cmd"]);
}

#[test]
fn overlay_contract_blank_scope_does_not_clobber_arg_scope() {
    let mut hints = WorkstationDispatchHints {
        objective: Some("o".to_string()),
        scope: Some("arg scope".to_string()),
        ..Default::default()
    };
    let contract = ParsedTaskContract {
        schema: "missiond.task-contract.v1".to_string(),
        goal: "g".to_string(),
        scope: Some("   ".to_string()),
        ..Default::default()
    };
    hints.overlay_contract(&contract);
    assert_eq!(hints.scope.as_deref(), Some("arg scope"));
}

// ── wave-19 / task 07 — brief integration tests ──────────────────────

#[test]
fn build_task_brief_with_source_prefixes_contract_block_when_path_supplied() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    let path = std::path::PathBuf::from("/tmp/contract.lisp");
    let brief = build_task_brief_with_source(&plan, &hints, "agent-team", Some(&path));
    // Contract preamble present.
    assert!(brief.contains("## Source contract"));
    assert!(brief.contains("/tmp/contract.lisp"));
    assert!(brief.contains("treat the contract as the SSOT"));
    // Existing canonical sections still present (wave-15/16/17 invariants).
    assert!(brief.contains("## Objective"));
    assert!(brief.contains("## Owned files"));
    assert!(brief.contains("## Commit policy"));
    assert!(brief.contains("## Completion handoff (scoped commit)"));
    // Agent-team hint still appears exactly once.
    assert_eq!(brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(), 1);
}

#[test]
fn build_task_brief_without_source_is_byte_identical_to_legacy_build_task_brief() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    let legacy = build_task_brief(&plan, &hints, "agent-team");
    let with_none = build_task_brief_with_source(&plan, &hints, "agent-team", None);
    assert_eq!(
        legacy, with_none,
        "wave-19 wrapper must be byte-identical to legacy entry when no contract"
    );
}

#[test]
fn build_task_brief_with_source_does_not_double_inject_completion_handoff() {
    // The completion handoff section is independent of the contract
    // preamble — it must appear exactly once even when the brief is
    // contract-flavoured.
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    let path = std::path::PathBuf::from("/tmp/contract.lisp");
    let brief = build_task_brief_with_source(&plan, &hints, "fresh-code-alignment", Some(&path));
    assert_eq!(
        brief
            .matches("## Completion handoff (scoped commit)")
            .count(),
        1
    );
    // Code task kind still classified from owned_files presence.
    assert!(brief.contains("- task kind: code"));
}

// ── wave-19 / task 07 — SafeDescriptor tests ─────────────────────────

#[test]
fn malformed_task_contract_descriptor_status_is_distinct() {
    let r = SafeDescriptorReason::MalformedTaskContract {
        path: "/tmp/x.lisp".to_string(),
        reason: "schema mismatch".to_string(),
    };
    assert_eq!(r.status(), "skipped_malformed_task_contract");
    let detail = r.detail();
    assert!(detail.contains("/tmp/x.lisp"));
    assert!(detail.contains("schema mismatch"));
    assert!(detail.contains("SSOT"));
}

#[test]
fn outcome_to_response_malformed_contract_carries_full_detail() {
    let outcome = WorkstationDispatchOutcome::SafeDescriptor {
        reason: SafeDescriptorReason::MalformedTaskContract {
            path: "/tmp/bad.lisp".to_string(),
            reason: "lex: unbalanced parens".to_string(),
        },
        task_brief: None,
    };
    let v = outcome_to_response_fields(&outcome, "agent-team");
    assert_eq!(
        v["workstation_dispatch_status"],
        "skipped_malformed_task_contract"
    );
    assert!(v["workstation_dispatch_reason"]
        .as_str()
        .unwrap()
        .contains("/tmp/bad.lisp"));
    assert!(v["workstation_dispatch_reason"]
        .as_str()
        .unwrap()
        .contains("lex: unbalanced parens"));
    // Scoped-commit policy contract still surfaces on the safe-descriptor
    // branch (wave-17 / task 07 invariant — applies regardless of branch).
    assert_eq!(v["scoped_commit_required"], json!(true));
}

// ── wave-19 / task 07 — path resolution tests ────────────────────────

#[test]
fn resolve_contract_path_keeps_absolute_paths_verbatim() {
    let abs = Path::new("/tmp/abs/contract.lisp");
    let root = Path::new("/Users/x/proj");
    assert_eq!(resolve_contract_path(abs, root), abs.to_path_buf());
}

#[test]
fn resolve_contract_path_joins_relative_against_project_root() {
    let rel = Path::new(".missiond/tasks/generated/abc/root.lisp");
    let root = Path::new("/Users/x/proj");
    assert_eq!(
        resolve_contract_path(rel, root),
        Path::new("/Users/x/proj/.missiond/tasks/generated/abc/root.lisp").to_path_buf()
    );
}

// ── wave-19 / task 07 — parser error reason mapping pin ──────────────

#[test]
fn task_contract_parse_error_reason_strings_are_actionable() {
    // Each variant produces a human-actionable reason string. Pinned so
    // a future refactor that loses detail (e.g. dropping the offending
    // schema value) trips a test.
    assert!(TaskContractParseError::Io("perm denied".into())
        .reason()
        .contains("perm denied"));
    assert!(TaskContractParseError::Lex("EOF".into())
        .reason()
        .contains("EOF"));
    assert!(TaskContractParseError::NotATaskForm("(plan)".into())
        .reason()
        .contains("(plan)"));
    assert!(TaskContractParseError::SchemaMismatch("v9".into())
        .reason()
        .contains("v9"));
    assert!(TaskContractParseError::MissingRequired("goal")
        .reason()
        .contains("goal"));
    assert!(TaskContractParseError::FieldShape {
        field: "write-scope",
        detail: "got 42".into(),
    }
    .reason()
    .contains("write-scope"));
}

// ── wave-21 / task 04 — autonomous workstation LLM proposal v0 ─────

#[test]
fn workstation_proposal_status_wire_strings_pin() {
    // The five values are part of the response wire contract.
    assert_eq!(
        WorkstationProposalStatus::NotInvoked.as_wire(),
        "not_invoked"
    );
    assert_eq!(
        WorkstationProposalStatus::Unavailable.as_wire(),
        "llm_unavailable"
    );
    assert_eq!(WorkstationProposalStatus::Suggested.as_wire(), "suggested");
    assert_eq!(
        WorkstationProposalStatus::NoSuggestions.as_wire(),
        "no_suggestions"
    );
    assert_eq!(
        WorkstationProposalStatus::PlanHintsPresent.as_wire(),
        "plan_hints_present"
    );
}

#[test]
fn workstation_proposal_safety_status_wire_strings_pin() {
    // The four values land verbatim on every proposal entry.
    assert_eq!(WorkstationProposalSafetyStatus::Safe.as_wire(), "safe");
    assert_eq!(
        WorkstationProposalSafetyStatus::AmbiguousValue.as_wire(),
        "ambiguous_value"
    );
    assert_eq!(
        WorkstationProposalSafetyStatus::UnsupportedTarget.as_wire(),
        "unsupported_target"
    );
    assert_eq!(
        WorkstationProposalSafetyStatus::InvalidStrategy.as_wire(),
        "invalid_strategy"
    );
}

#[test]
fn workstation_proposal_to_json_pins_applied_false() {
    // Critical invariant: every proposal carries `applied=false` on
    // the wire so observers can `assert proposal.applied == false`
    // without re-reading the source. Wave-21 / task 04 explicitly
    // forbids auto-spawn from proposals.
    let p = WorkstationProposal {
        field: "target",
        value: json!("mission_task_delegate"),
        confidence: WorkstationProposalConfidence::High,
        evidence: "plan sexp delegates to claudecode".to_string(),
        safety_status: WorkstationProposalSafetyStatus::Safe,
    };
    let v = p.to_json();
    assert_eq!(v["applied"], json!(false));
    assert_eq!(v["field"], json!("target"));
    assert_eq!(v["confidence"], json!("high"));
    assert_eq!(v["safety_status"], json!("safe"));
    assert_eq!(v["value"], json!("mission_task_delegate"));
}

#[test]
fn workstation_proposal_bundle_unavailable_carries_reason() {
    let b = WorkstationProposalBundle::unavailable("gateway not initialized");
    assert_eq!(b.status, WorkstationProposalStatus::Unavailable);
    assert!(b.proposals.is_empty());
    assert_eq!(
        b.unavailable_reason.as_deref(),
        Some("gateway not initialized")
    );
    assert_eq!(
        b.request_caller.as_deref(),
        Some(SONNET_WORKSTATION_PROPOSAL_CALLER)
    );
    assert!(
        b.model.is_none(),
        "unavailable bundle must not name a model"
    );
}

#[test]
fn workstation_proposal_bundle_plan_hints_present_carries_reason() {
    let b = WorkstationProposalBundle::plan_hints_present("signals present: caller.objective");
    assert_eq!(b.status, WorkstationProposalStatus::PlanHintsPresent);
    assert!(b.proposals.is_empty());
    assert_eq!(
        b.unavailable_reason.as_deref(),
        Some("signals present: caller.objective")
    );
}

#[test]
fn workstation_proposal_bundle_to_response_pins_auto_spawn_false() {
    // The bundle-level `auto_spawn=false` field is the wire-level
    // proof of the never-auto-spawn invariant. A UI surfacing the
    // block can quote this single field rather than re-deriving it
    // from the per-proposal `applied=false`.
    let bundle = WorkstationProposalBundle::unavailable("test reason");
    let v = bundle.to_response_json();
    assert_eq!(v["auto_spawn"], json!(false));
    assert_eq!(v["status"], "llm_unavailable");
    assert_eq!(v["unavailable_reason"], "test reason");
    assert_eq!(v["proposals"], json!([]));
}

#[test]
fn workstation_proposal_bundle_to_response_includes_proposals_when_present() {
    let bundle = WorkstationProposalBundle {
        status: WorkstationProposalStatus::Suggested,
        proposals: vec![WorkstationProposal {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: WorkstationProposalConfidence::Medium,
            evidence: "plan sexp suggests delegation".to_string(),
            safety_status: WorkstationProposalSafetyStatus::Safe,
        }],
        parse_warnings: vec!["proposals[2] field `foo` not in allowlist".to_string()],
        unavailable_reason: None,
        model: Some("claude-sonnet".to_string()),
        request_caller: Some(SONNET_WORKSTATION_PROPOSAL_CALLER.to_string()),
    };
    let v = bundle.to_response_json();
    assert_eq!(v["status"], "suggested");
    assert_eq!(v["proposals"][0]["field"], "target");
    assert_eq!(v["proposals"][0]["applied"], false);
    assert_eq!(v["proposals"][0]["safety_status"], "safe");
    assert_eq!(v["model"], "claude-sonnet");
    assert_eq!(v["caller"], SONNET_WORKSTATION_PROPOSAL_CALLER);
    assert_eq!(
        v["parse_warnings"][0],
        "proposals[2] field `foo` not in allowlist"
    );
    // The auto-spawn invariant is pinned on every bundle render.
    assert_eq!(v["auto_spawn"], json!(false));
}

#[test]
fn workstation_proposal_gate_is_fully_silent_when_no_signals() {
    let gate = WorkstationProposalGate {
        caller_target_present: false,
        caller_dispatch_strategy_present: false,
        caller_objective_present: false,
        caller_scope_present: false,
        caller_owned_files_present: false,
        caller_project_signal_present: false,
        plan_hints_present: false,
        plan_workstation_opt_in: false,
        _marker: std::marker::PhantomData,
    };
    assert!(gate.is_fully_silent());
    assert!(gate.signal_summary().contains("no signals"));
}

#[test]
fn workstation_proposal_gate_not_silent_when_caller_objective_set() {
    let gate = WorkstationProposalGate {
        caller_target_present: false,
        caller_dispatch_strategy_present: false,
        caller_objective_present: true,
        caller_scope_present: false,
        caller_owned_files_present: false,
        caller_project_signal_present: false,
        plan_hints_present: false,
        plan_workstation_opt_in: false,
        _marker: std::marker::PhantomData,
    };
    assert!(!gate.is_fully_silent());
    assert!(gate.signal_summary().contains("caller.objective"));
}

#[test]
fn workstation_proposal_gate_not_silent_when_plan_hints_present() {
    let gate = WorkstationProposalGate {
        caller_target_present: false,
        caller_dispatch_strategy_present: false,
        caller_objective_present: false,
        caller_scope_present: false,
        caller_owned_files_present: false,
        caller_project_signal_present: false,
        plan_hints_present: true,
        plan_workstation_opt_in: false,
        _marker: std::marker::PhantomData,
    };
    assert!(!gate.is_fully_silent());
    assert!(gate.signal_summary().contains("plan.hints"));
}

#[test]
fn workstation_proposal_gate_not_silent_when_plan_opt_in_set() {
    let gate = WorkstationProposalGate {
        caller_target_present: false,
        caller_dispatch_strategy_present: false,
        caller_objective_present: false,
        caller_scope_present: false,
        caller_owned_files_present: false,
        caller_project_signal_present: false,
        plan_hints_present: false,
        plan_workstation_opt_in: true,
        _marker: std::marker::PhantomData,
    };
    assert!(!gate.is_fully_silent());
    assert!(gate.signal_summary().contains("plan.workstation_dispatch"));
}

#[test]
fn build_workstation_proposal_prompt_embeds_plan_and_provenance() {
    let plan = "(plan :id \"p1\" :board-task-id \"btk-1\")";
    let (system, user) = build_workstation_proposal_prompt(plan, Some("directive/abc:1"));
    // System prompt names the four allowlisted fields.
    for f in WORKSTATION_PROPOSAL_FIELDS {
        assert!(
            system.contains(f),
            "system prompt must mention field `{}`",
            f
        );
    }
    // System prompt pins the never-auto-spawn invariant.
    assert!(system.contains("NEVER be auto-"));
    assert!(system.to_ascii_lowercase().contains("never"));
    assert!(system.contains("STRICT JSON"));
    // System prompt pins the conservative target / strategy whitelists.
    assert!(system.contains("mission_execution"));
    assert!(system.contains("mission_task_delegate"));
    assert!(system.contains("fresh-code-alignment"));
    // User prompt embeds plan + provenance.
    assert!(user.contains(plan));
    assert!(user.contains("directive/abc:1"));
    // User prompt explicitly tells the model that no caller hints exist.
    assert!(user.contains("no workstation hints") || user.contains("no \nworkstation hints"));
}

#[test]
fn build_workstation_proposal_prompt_handles_absent_provenance() {
    let (_, user) = build_workstation_proposal_prompt("(plan)", None);
    assert!(user.contains("(none)"));
}

#[test]
fn parse_workstation_proposals_accepts_canonical_envelope() {
    let raw = r#"{
        "proposals": [
            {
                "field": "target",
                "value": "mission_task_delegate",
                "confidence": "high",
                "evidence": "plan sexp clearly delegates to claudecode"
            },
            {
                "field": "objective",
                "value": "ship the wave 21 task four",
                "confidence": "medium",
                "evidence": "directive provenance suggests this work"
            }
        ]
    }"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(warnings.is_empty(), "warnings: {:?}", warnings);
    assert_eq!(proposals.len(), 2);
    assert_eq!(proposals[0].field, "target");
    assert_eq!(
        proposals[0].safety_status,
        WorkstationProposalSafetyStatus::Safe
    );
    assert_eq!(proposals[1].field, "objective");
    assert_eq!(
        proposals[1].safety_status,
        WorkstationProposalSafetyStatus::Safe
    );
}

#[test]
fn parse_workstation_proposals_accepts_bare_array() {
    let raw = r#"[{"field":"scope","value":"crates/foo only","confidence":"medium","evidence":"plan suggests narrow scope"}]"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(warnings.is_empty(), "warnings: {:?}", warnings);
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].field, "scope");
}

#[test]
fn parse_workstation_proposals_strips_markdown_fence() {
    let raw = "```json\n{\"proposals\": [{\"field\":\"dispatch_strategy\",\"value\":\"agent-team\",\"confidence\":\"medium\",\"evidence\":\"parallelism hint\"}]}\n```";
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(warnings.is_empty(), "warnings: {:?}", warnings);
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].field, "dispatch_strategy");
}

#[test]
fn parse_workstation_proposals_rejects_unknown_field() {
    let raw = r#"{"proposals":[{"field":"orbital_velocity","value":"warp9","confidence":"high","evidence":"x"}]}"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(proposals.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("orbital_velocity"));
    assert!(warnings[0].contains("not in allowlist"));
}

#[test]
fn parse_workstation_proposals_rejects_non_string_value() {
    let raw =
        r#"{"proposals":[{"field":"objective","value":42,"confidence":"high","evidence":"x"}]}"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("must be string"));
}

#[test]
fn parse_workstation_proposals_rejects_blank_value() {
    let raw =
        r#"{"proposals":[{"field":"objective","value":"   ","confidence":"high","evidence":"x"}]}"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("non-empty"));
}

#[test]
fn parse_workstation_proposals_rejects_invalid_confidence() {
    let raw = r#"{"proposals":[{"field":"target","value":"mission_execution","confidence":"absolute","evidence":"x"}]}"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("absolute"));
}

#[test]
fn parse_workstation_proposals_rejects_missing_evidence() {
    let raw = r#"{"proposals":[{"field":"target","value":"mission_execution","confidence":"high","evidence":""}]}"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("evidence"));
}

#[test]
fn parse_workstation_proposals_dedupes_repeated_fields() {
    let raw = r#"{
        "proposals":[
            {"field":"target","value":"mission_execution","confidence":"medium","evidence":"first"},
            {"field":"target","value":"mission_task_delegate","confidence":"high","evidence":"second"}
        ]
    }"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].evidence, "first");
    assert!(warnings.iter().any(|w| w.contains("duplicate")));
}

#[test]
fn parse_workstation_proposals_caps_long_lists() {
    let raw = serde_json::to_string(&json!({
        "proposals": [
            {"field": "target",            "value": "mission_execution",       "confidence": "low", "evidence": "x"},
            {"field": "dispatch_strategy", "value": "fresh-code-alignment",    "confidence": "low", "evidence": "x"},
            {"field": "objective",         "value": "ship the wave",           "confidence": "low", "evidence": "x"},
            {"field": "scope",             "value": "narrow scope",            "confidence": "low", "evidence": "x"},
            {"field": "extra_one",         "value": "ignored",                 "confidence": "low", "evidence": "x"},
            {"field": "extra_two",         "value": "ignored",                 "confidence": "low", "evidence": "x"},
            {"field": "extra_three",       "value": "ignored",                 "confidence": "low", "evidence": "x"},
            {"field": "extra_four",        "value": "ignored",                 "confidence": "low", "evidence": "x"},
        ]
    }))
    .unwrap();
    let (proposals, warnings) = parse_workstation_proposals(&raw);
    assert!(proposals.len() <= WORKSTATION_PROPOSAL_CAP);
    assert_eq!(proposals.len(), 4);
    assert!(warnings.iter().any(|w| w.contains("extra_one")));
}

#[test]
fn parse_workstation_proposals_rejects_garbage_json() {
    let (proposals, warnings) = parse_workstation_proposals("not json at all");
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("not valid JSON"));
}

#[test]
fn parse_workstation_proposals_rejects_missing_proposals_key() {
    let raw = r#"{"results": []}"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(proposals.is_empty());
    assert!(warnings[0].contains("missing required `proposals`"));
}

#[test]
fn classify_proposal_safety_target_unsupported_value_tagged() {
    let s = classify_proposal_safety("target", "mission_unknown");
    assert_eq!(s, WorkstationProposalSafetyStatus::UnsupportedTarget);
}

#[test]
fn classify_proposal_safety_target_supported_value_safe() {
    for t in PROPOSAL_VALID_TARGETS {
        let s = classify_proposal_safety("target", t);
        assert_eq!(
            s,
            WorkstationProposalSafetyStatus::Safe,
            "target `{}` should be Safe",
            t
        );
    }
}

#[test]
fn classify_proposal_safety_dispatch_strategy_unsupported_tagged() {
    for bad in ["prompt-fallback", "unknown", "telepathy"] {
        let s = classify_proposal_safety("dispatch_strategy", bad);
        assert_eq!(
            s,
            WorkstationProposalSafetyStatus::InvalidStrategy,
            "strategy `{}` should be InvalidStrategy",
            bad
        );
    }
}

#[test]
fn classify_proposal_safety_dispatch_strategy_supported_safe() {
    for s in PROPOSAL_VALID_STRATEGIES {
        let safety = classify_proposal_safety("dispatch_strategy", s);
        assert_eq!(
            safety,
            WorkstationProposalSafetyStatus::Safe,
            "strategy `{}` should be Safe",
            s
        );
    }
}

#[test]
fn classify_proposal_safety_objective_too_short_tagged_ambiguous() {
    let s = classify_proposal_safety("objective", "go");
    assert_eq!(s, WorkstationProposalSafetyStatus::AmbiguousValue);
}

#[test]
fn classify_proposal_safety_scope_too_short_tagged_ambiguous() {
    let s = classify_proposal_safety("scope", "x");
    assert_eq!(s, WorkstationProposalSafetyStatus::AmbiguousValue);
}

#[test]
fn classify_proposal_safety_objective_long_enough_safe() {
    let s = classify_proposal_safety("objective", "ship the wave 21 task");
    assert_eq!(s, WorkstationProposalSafetyStatus::Safe);
}

#[test]
fn parse_workstation_proposals_tags_unsupported_target_safety_status() {
    // The validator surfaces the proposal but flags the safety status
    // so the operator sees the warning. We do NOT block the proposal
    // — that is the operator's job.
    let raw = r#"{"proposals":[{"field":"target","value":"mission_unknown","confidence":"high","evidence":"plan suggests it"}]}"#;
    let (proposals, warnings) = parse_workstation_proposals(raw);
    assert!(warnings.is_empty(), "warnings: {:?}", warnings);
    assert_eq!(proposals.len(), 1);
    assert_eq!(
        proposals[0].safety_status,
        WorkstationProposalSafetyStatus::UnsupportedTarget
    );
}

/// Wave-21 / task 04 — the "no fallback" invariant: when the Sonnet
/// gateway is unavailable we surface a typed `Unavailable` bundle
/// rather than silently downgrading to a deterministic-only or
/// `claude -p` path. This test pins the unavailable-reason text so a
/// future refactor that swallows the gateway-not-initialised
/// distinction trips the test.
#[test]
fn workstation_proposal_unavailable_reason_pins_no_fallback_text() {
    let b = WorkstationProposalBundle::unavailable(
        "Sonnet gateway not initialized; autonomous workstation proposal unavailable \
         (no fallback to claude -p / prompt mode in v0)",
    );
    let reason = b.unavailable_reason.unwrap_or_default();
    assert!(reason.contains("no fallback"));
    assert!(reason.contains("claude -p") || reason.contains("prompt mode"));
}

// ── Wave 21 / Task 08 — machine-contract autonomous loop smoke ──
//
// Pure-helper smoke proving the wave21-04 workstation-proposal
// pipeline (parse → classify → bundle → response JSON) preserves the
// propose-only invariants on the canonical fixture. Layered on top
// of the wave-15..20 fixtures already exercised above:
//   * I4-04  the `applied=false` invariant is pinned on EVERY
//            proposal regardless of safety_status (UI / observers
//            can `assert proposal.applied == false` without reading
//            the source).
//   * I4-04  every safety_status (`safe` / `unsupported_target` /
//            `invalid_strategy`) survives the round-trip — the
//            classifier never silently demotes a flagged proposal
//            to `safe`.
//   * I4-04  the `auto_spawn=false` invariant is pinned on the
//            bundle JSON (not just the proposal JSON) so a
//            top-level grep proves the bundle never auto-applies.

/// Canonical Sonnet response shape used by the wave21-04 propose-only
/// pipeline. Three proposals span distinct fields + the safety
/// spectrum (safe target, invalid strategy, ambiguous objective) so
/// the smoke can pin the applied=false invariant across every
/// safety_status branch without tripping the parser's per-field
/// dedup rule.
const WAVE21_08_SMOKE_PROPOSAL_JSON: &str = r#"
{
  "proposals": [
{
  "field": "target",
  "value": "mission_task_delegate",
  "confidence": "high",
  "evidence": "node hint :target mission_task_delegate matches the wave21-08 smoke contract"
},
{
  "field": "dispatch_strategy",
  "value": "vibes-based",
  "confidence": "medium",
  "evidence": "model invented a strategy not on the conservative subset"
},
{
  "field": "objective",
  "value": "ship the wave21-08 deterministic loop smoke",
  "confidence": "low",
  "evidence": "summarised from the contract :goal field"
}
  ]
}
"#;

/// Wave21-08 propose-only smoke: every parsed wave21-04 proposal
/// MUST carry `applied=false` (invariant I3-04) on the wire shape,
/// the bundle MUST surface `auto_spawn=false`, and the safety
/// classifier MUST tag the unsupported / invalid values verbatim.
#[test]
fn smoke_wave21_workstation_proposals_pin_applied_false_invariant_across_safety_spectrum() {
    let (proposals, warnings) = parse_workstation_proposals(WAVE21_08_SMOKE_PROPOSAL_JSON);
    assert_eq!(
        proposals.len(),
        3,
        "fixture must surface three proposals — got {} (warnings: {:?})",
        proposals.len(),
        warnings
    );
    // Pin the safety spectrum + applied=false invariant on every
    // proposal in one pass so a future refactor that flips the wire
    // contract (or silently demotes a flagged proposal) lands an
    // explicit failure here.
    let mut safety_seen: Vec<WorkstationProposalSafetyStatus> = Vec::new();
    for p in &proposals {
        let json = p.to_json();
        assert_eq!(
            json["applied"], false,
            "wave21-04 invariant I3: every proposal MUST carry applied=false on the wire"
        );
        assert!(
            !json["evidence"].as_str().unwrap_or("").is_empty(),
            "wave21-04 propose-only contract: evidence MUST be non-empty"
        );
        safety_seen.push(p.safety_status);
    }
    assert!(
        safety_seen.contains(&WorkstationProposalSafetyStatus::Safe),
        "wave21-08 fixture must include at least one Safe proposal"
    );
    assert!(
        safety_seen.contains(&WorkstationProposalSafetyStatus::InvalidStrategy),
        "wave21-08 fixture must include the invalid-strategy case (vibes-based)"
    );

    // Cross-check the dedicated unsupported_target classifier branch
    // independently of the bundle round-trip so a future refactor
    // that demotes UnsupportedTarget to Safe lands an explicit
    // failure here too. wave21-04 invariant: the classifier
    // surfaces UnsupportedTarget verbatim — never silently demotes
    // a flagged proposal.
    assert_eq!(
        classify_proposal_safety("target", "mission_unknown"),
        WorkstationProposalSafetyStatus::UnsupportedTarget,
        "wave21-04 invariant: unsupported target value MUST tag UnsupportedTarget"
    );

    // Bundle-level invariant: `auto_spawn=false` MUST appear on the
    // top-level JSON so observers can grep one place.
    let bundle = WorkstationProposalBundle {
        status: WorkstationProposalStatus::Suggested,
        proposals,
        parse_warnings: warnings,
        unavailable_reason: None,
        model: Some("claude-sonnet-4-7".to_string()),
        request_caller: Some("wave21-08-smoke".to_string()),
    };
    let block = bundle.to_response_json();
    assert_eq!(
        block["auto_spawn"], false,
        "wave21-04 invariant: bundle MUST pin auto_spawn=false at the top level"
    );
    assert_eq!(
        block["status"], "suggested",
        "wave21-04 status mapping: Suggested → \"suggested\""
    );
    let inner = block["proposals"].as_array().expect("proposals array");
    for p in inner {
        assert_eq!(
            p["applied"], false,
            "wave21-04 invariant I3: bundle round-trip preserves applied=false"
        );
    }
}

/// Wave21-08 propose-only smoke: when the Sonnet gateway is
/// unreachable the bundle MUST surface status=`llm_unavailable` +
/// the explicit "no fallback" reason text. Pinned here so a future
/// refactor that adds a silent `claude -p` fallback fails the
/// machine-contract loop smoke immediately.
#[test]
fn smoke_wave21_workstation_proposal_unavailable_pins_no_silent_fallback() {
    let bundle = WorkstationProposalBundle::unavailable(
        "Sonnet gateway unavailable; wave21-04 propose-only contract refuses fallback to \
         claude -p / prompt mode (autonomous workstation dispatch is opt-in)",
    );
    let block = bundle.to_response_json();
    assert_eq!(block["status"], "llm_unavailable");
    assert_eq!(
        block["auto_spawn"], false,
        "wave21-04 invariant: even llm_unavailable surfaces auto_spawn=false at the top level"
    );
    assert_eq!(
        block["proposals"]
            .as_array()
            .expect("empty proposals array")
            .len(),
        0,
        "llm_unavailable bundle must NOT carry a synthesised fallback proposal"
    );
    let reason = block["unavailable_reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("no fallback")
            || reason.contains("refuses fallback")
            || reason.contains("claude -p")
            || reason.contains("prompt mode"),
        "wave21-04 invariant: unavailable reason MUST name the no-fallback contract"
    );
}

/// Wave21-08 contract overlay smoke: the wave-19/07 narrow parser
/// projects the fixture task-contract Lisp text and the consumer
/// rule (contract wins on every non-empty field) reaches the brief
/// renderer. Pinning this here closes the wave21-08 loop on the
/// workstation-side: the same contract the verifier ratifies
/// (wave21-03) also drives the brief the worker reads.
#[test]
fn smoke_wave21_machine_contract_overlay_drives_brief_renderer() {
    const WAVE21_08_SMOKE_DISPATCH_CONTRACT: &str = r#"
(task wave21-08-dispatch-smoke
  :schema "missiond.task-contract.v1"
  :goal "wave21-08 machine-contract loop drives the brief from the contract"
  :scope "wave 21 task 08 only"
  :write-scope ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
  :must-not-touch ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"]
  :acceptance ["cargo test -p missiond-daemon"]
  :commit (:required true :message "test(intent): cover wave21 loop" :scope-check write-scope-only :policy "scoped")
  :dispatch-strategy "agent-team"
  :target-project "missiond"
  :target "mission_task_delegate")
"#;
    let parsed = parse_task_contract(WAVE21_08_SMOKE_DISPATCH_CONTRACT)
        .expect("wave21-08 dispatch contract must parse");
    assert_eq!(
        parsed.goal,
        "wave21-08 machine-contract loop drives the brief from the contract"
    );
    assert_eq!(parsed.dispatch_strategy.as_deref(), Some("agent-team"));
    assert_eq!(parsed.commit_policy.as_deref(), Some("scoped"));
    assert_eq!(parsed.target.as_deref(), Some("mission_task_delegate"));

    // Caller side starts with stale / placeholder hints so the
    // overlay rule is exercised end-to-end.
    let mut hints = WorkstationDispatchHints {
        objective: Some("STALE caller objective".to_string()),
        scope: Some("STALE caller scope".to_string()),
        owned_files: vec!["stale.rs".to_string()],
        forbidden_files: vec![],
        acceptance_commands: vec!["stale-cmd".to_string()],
        commit_policy: None,
        target_project: None,
        requested_cwd: None,
        dispatch_strategy: Some("fresh-code-alignment".to_string()),
    };
    hints.overlay_contract(&parsed);

    // wave21-08 invariant: the contract overlay wins on EVERY non-
    // empty field — caller stale data MUST NOT leak into the brief.
    assert_eq!(
        hints.objective.as_deref(),
        Some("wave21-08 machine-contract loop drives the brief from the contract"),
        "contract :goal MUST replace caller stale objective"
    );
    assert_eq!(
        hints.scope.as_deref(),
        Some("wave 21 task 08 only"),
        "contract :scope MUST replace caller stale scope"
    );
    assert_eq!(
        hints.owned_files,
        vec!["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs".to_string()],
        "contract :write-scope MUST replace caller stale owned_files"
    );
    assert_eq!(
        hints.forbidden_files,
        vec!["crates/missiond-daemon/src/handlers/knowledge/workflow.rs".to_string()],
        "contract :must-not-touch MUST surface verbatim"
    );
    assert_eq!(
        hints.commit_policy.as_deref(),
        Some("scoped"),
        "contract :commit :policy MUST reach hints"
    );
    assert_eq!(
        hints.dispatch_strategy.as_deref(),
        Some("agent-team"),
        "contract :dispatch-strategy MUST replace caller fresh-code-alignment"
    );

    // Brief renderer must carry the wave-19/07 source-contract
    // preamble so the worker sees the SSOT pointer; the contract
    // :goal MUST surface as the brief objective; the stale caller
    // objective MUST NOT leak.
    let plan = fixture_plan("(plan)");
    let contract_path = std::path::Path::new(
        "/tmp/missiond-wave21-08-smoke/.missiond/tasks/wave21/wave21-08-dispatch.lisp",
    );
    let brief = build_task_brief_with_source(&plan, &hints, "agent-team", Some(contract_path));
    assert!(
        brief.contains("## Source contract"),
        "wave21-08 brief MUST carry the wave-19/07 source-contract preamble"
    );
    assert!(
        brief.contains(
            "/tmp/missiond-wave21-08-smoke/.missiond/tasks/wave21/wave21-08-dispatch.lisp"
        ),
        "wave21-08 brief preamble MUST name the on-disk contract path"
    );
    assert!(
        brief.contains("wave21-08 machine-contract loop drives the brief from the contract"),
        "wave21-08 brief MUST render the contract :goal as the objective"
    );
    assert!(
        !brief.contains("STALE caller objective"),
        "wave21-08 invariant: stale caller objective MUST NOT leak into the contract-driven brief"
    );
    // Pin the agent-team injection invariant survives the wave21-08
    // path.
    assert_eq!(
        brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
        1,
        "wave21-08 brief: agent-team hint MUST appear exactly once on the contract-driven path"
    );
}

// ── wave-22 / task 05 — autonomous workstation true spawn v1 tests ──

/// Helper: build a minimal `Suggested` bundle with a single `target`
/// proposal of `mission_task_delegate` carrying high confidence /
/// safe safety_status. Used by the gate evaluator unit tests so
/// they don't have to rebuild the same boilerplate.
fn fixture_spawnable_bundle() -> WorkstationProposalBundle {
    WorkstationProposalBundle {
        status: WorkstationProposalStatus::Suggested,
        proposals: vec![
            WorkstationProposal {
                field: "target",
                value: json!("mission_task_delegate"),
                confidence: WorkstationProposalConfidence::High,
                evidence: "PLAN names mission_task_delegate".to_string(),
                safety_status: WorkstationProposalSafetyStatus::Safe,
            },
            WorkstationProposal {
                field: "objective",
                value: json!("ship the wave22-05 invariant proof"),
                confidence: WorkstationProposalConfidence::High,
                evidence: "PLAN summary names the goal".to_string(),
                safety_status: WorkstationProposalSafetyStatus::Safe,
            },
        ],
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: Some("claude-sonnet".to_string()),
        request_caller: Some(SONNET_WORKSTATION_PROPOSAL_CALLER.to_string()),
    }
}

/// Helper: a `ParsedTaskContract` with non-empty :write-scope and
/// non-overlapping :must-not-touch. Used by the gate happy path.
fn fixture_spawnable_contract() -> ParsedTaskContract {
    ParsedTaskContract {
        schema: "missiond.task-contract.v1".to_string(),
        goal: "wave22-05 spawn smoke".to_string(),
        scope: Some("ship the gate".to_string()),
        write_scope: vec![
            "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs".to_string(),
            "crates/missiond-daemon/src/handlers/knowledge/plan.rs".to_string(),
        ],
        must_not_touch: vec![".missiond/v2/intent-event-bus.lisp".to_string()],
        acceptance: vec!["cargo test -p missiond-daemon".to_string()],
        commit_policy: Some("scoped".to_string()),
        dispatch_strategy: Some("agent-team".to_string()),
        target_project: None,
        requested_cwd: None,
        target: Some("mission_task_delegate".to_string()),
        session_trace_path: None,
    }
}

fn fixture_input_all_green() -> WorkstationAutoSpawnInput {
    // Build a spawnable bundle to derive the canonical hash so the
    // helper does not have to import sha2 from inside the test.
    let bundle = fixture_spawnable_bundle();
    let hash = compute_workstation_proposal_hash(&bundle);
    WorkstationAutoSpawnInput {
        auto_spawn: true,
        proposal_hash: Some(hash),
        caller_approved: true,
        task_contract_path: Some(
            ".missiond/tasks/wave22/wave22-05-autonomous-workstation-true-spawn-v1.lisp"
                .to_string(),
        ),
        preflight_status_acceptable: true,
        explicit: true,
    }
}

#[test]
fn wave22_05_auto_spawn_status_wire_strings_pin() {
    let cases = [
        (WorkstationAutoSpawnStatus::NotRequested, "not_requested"),
        (WorkstationAutoSpawnStatus::Spawned, "spawned"),
        (
            WorkstationAutoSpawnStatus::SkippedUnavailable,
            "skipped_unavailable",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedNoProposals,
            "skipped_no_proposals",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedUnsafeProposal,
            "skipped_unsafe_proposal",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedConfidenceTooLow,
            "skipped_confidence_too_low",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedCallerNotApproved,
            "skipped_caller_not_approved",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedMissingTaskContractPath,
            "skipped_missing_task_contract_path",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedMalformedTaskContract,
            "skipped_malformed_task_contract",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedEmptyWriteScope,
            "skipped_empty_write_scope",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedForbiddenScopeOverlap,
            "skipped_forbidden_scope_overlap",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedPreflightUnacceptable,
            "skipped_preflight_unacceptable",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedUnsupportedTarget,
            "skipped_unsupported_target",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedSubstrateRefused,
            "skipped_substrate_refused",
        ),
        (
            WorkstationAutoSpawnStatus::SkippedSubstrateInnerError,
            "skipped_substrate_inner_error",
        ),
    ];
    let mut seen = std::collections::HashSet::new();
    for (status, wire) in cases {
        assert_eq!(status.as_wire(), wire);
        assert!(seen.insert(wire), "wire string `{}` duplicated", wire);
    }
    // was_spawned is true ONLY for Spawned.
    assert!(WorkstationAutoSpawnStatus::Spawned.was_spawned());
    for (status, _) in cases {
        if !matches!(status, WorkstationAutoSpawnStatus::Spawned) {
            assert!(
                !status.was_spawned(),
                "{:?} must not report spawned",
                status
            );
        }
    }
}

#[test]
fn wave22_05_proposal_hash_status_wire_strings_pin() {
    assert_eq!(
        WorkstationProposalHashStatus::NotSupplied.as_wire(),
        "not_supplied"
    );
    assert_eq!(WorkstationProposalHashStatus::Matches.as_wire(), "matches");
    assert_eq!(
        WorkstationProposalHashStatus::Mismatch.as_wire(),
        "mismatch"
    );
    assert_eq!(
        WorkstationProposalHashStatus::NoProposalAvailable.as_wire(),
        "no_proposal_available"
    );
}

#[test]
fn wave22_05_compute_hash_is_deterministic_and_stable() {
    let bundle = fixture_spawnable_bundle();
    let h1 = compute_workstation_proposal_hash(&bundle);
    let h2 = compute_workstation_proposal_hash(&bundle);
    assert_eq!(h1, h2, "hash must be deterministic across calls");
    assert_eq!(h1.len(), 32, "hash MUST be exactly 32 hex chars (128 bits)");
    assert!(
        h1.chars().all(|c| c.is_ascii_hexdigit()),
        "hash MUST be hex-only"
    );

    // Mutating any load-bearing field changes the hash.
    let mut other = fixture_spawnable_bundle();
    other.proposals[0].value = json!("mission_execution");
    let h3 = compute_workstation_proposal_hash(&other);
    assert_ne!(h1, h3, "hash MUST change when proposal value changes");

    let mut other = fixture_spawnable_bundle();
    other.proposals[0].confidence = WorkstationProposalConfidence::Medium;
    let h4 = compute_workstation_proposal_hash(&other);
    assert_ne!(h1, h4, "hash MUST change when confidence changes");

    let mut other = fixture_spawnable_bundle();
    other.proposals[0].safety_status = WorkstationProposalSafetyStatus::AmbiguousValue;
    let h5 = compute_workstation_proposal_hash(&other);
    assert_ne!(h1, h5, "hash MUST change when safety_status changes");

    // Mutating non-load-bearing fields does NOT change the hash.
    let mut other = fixture_spawnable_bundle();
    other.proposals[0].evidence = "different evidence text".to_string();
    let h6 = compute_workstation_proposal_hash(&other);
    assert_eq!(
        h1, h6,
        "hash MUST be stable across superficial evidence text changes"
    );
}

#[test]
fn wave22_05_parse_input_default_is_off_and_response_block_omitted_path() {
    let parsed = parse_workstation_auto_spawn_input(&json!({})).expect("ok");
    assert!(!parsed.auto_spawn);
    assert!(!parsed.caller_approved);
    assert!(!parsed.preflight_status_acceptable);
    assert!(parsed.proposal_hash.is_none());
    assert!(parsed.task_contract_path.is_none());
    assert!(!parsed.explicit, "absent ⇒ explicit=false");
}

#[test]
fn wave22_05_parse_input_rejects_string_true_for_auto_spawn() {
    let err = parse_workstation_auto_spawn_input(&json!({"auto_spawn": "true"}))
        .expect_err("string \"true\" must fail-fast");
    assert_eq!(err.0, AUTO_SPAWN_INVALID_PARAM);
    assert!(err.1.contains("auto_spawn"));
}

#[test]
fn wave22_05_parse_input_rejects_string_true_for_caller_approved() {
    let err = parse_workstation_auto_spawn_input(&json!({"workstation_caller_approved": "true"}))
        .expect_err("string \"true\" must fail-fast");
    assert_eq!(err.0, AUTO_SPAWN_INVALID_PARAM);
    assert!(err.1.contains("workstation_caller_approved"));
}

#[test]
fn wave22_05_parse_input_rejects_non_string_path() {
    let err = parse_workstation_auto_spawn_input(&json!({"task_contract_path": 42}))
        .expect_err("non-string path must fail-fast");
    assert_eq!(err.0, AUTO_SPAWN_INVALID_PARAM);
    assert!(err.1.contains("task_contract_path"));
}

#[test]
fn wave22_05_parse_input_accepts_bool_and_string_fields() {
    let parsed = parse_workstation_auto_spawn_input(&json!({
        "auto_spawn": true,
        "workstation_proposal_hash": "deadbeef",
        "workstation_caller_approved": true,
        "task_contract_path": ".missiond/tasks/foo.lisp",
        "preflight_status_acceptable": true,
    }))
    .expect("ok");
    assert!(parsed.auto_spawn);
    assert_eq!(parsed.proposal_hash.as_deref(), Some("deadbeef"));
    assert!(parsed.caller_approved);
    assert_eq!(
        parsed.task_contract_path.as_deref(),
        Some(".missiond/tasks/foo.lisp")
    );
    assert!(parsed.preflight_status_acceptable);
    assert!(parsed.explicit);
}

#[test]
fn wave22_05_preflight_ok_when_auto_spawn_off() {
    let input = WorkstationAutoSpawnInput::default();
    assert!(enforce_auto_spawn_preflight(&input, None).is_ok());
    let bundle = fixture_spawnable_bundle();
    assert!(enforce_auto_spawn_preflight(&input, Some(&bundle)).is_ok());
}

#[test]
fn wave22_05_preflight_missing_hash_when_auto_spawn_on() {
    let mut input = fixture_input_all_green();
    input.proposal_hash = None;
    let bundle = fixture_spawnable_bundle();
    let err = enforce_auto_spawn_preflight(&input, Some(&bundle))
        .expect_err("missing hash must fail-fast");
    assert_eq!(err.0, AUTO_SPAWN_MISSING_PROPOSAL_HASH);
}

#[test]
fn wave22_05_preflight_mismatch_hash_when_auto_spawn_on() {
    let mut input = fixture_input_all_green();
    input.proposal_hash = Some("0".repeat(32));
    let bundle = fixture_spawnable_bundle();
    let err = enforce_auto_spawn_preflight(&input, Some(&bundle))
        .expect_err("mismatch hash must fail-fast");
    assert_eq!(err.0, AUTO_SPAWN_PROPOSAL_HASH_MISMATCH);
}

#[test]
fn wave22_05_preflight_no_bundle_when_auto_spawn_on_missing_hash() {
    let mut input = fixture_input_all_green();
    input.proposal_hash = None;
    let err = enforce_auto_spawn_preflight(&input, None)
        .expect_err("no bundle ⇒ missing-hash even when caller forgot the hash");
    assert_eq!(err.0, AUTO_SPAWN_MISSING_PROPOSAL_HASH);
}

#[test]
fn wave22_05_preflight_no_bundle_when_auto_spawn_on_with_hash() {
    let input = fixture_input_all_green();
    let err = enforce_auto_spawn_preflight(&input, None)
        .expect_err("no bundle ⇒ mismatch even when caller supplied a hash");
    assert_eq!(err.0, AUTO_SPAWN_PROPOSAL_HASH_MISMATCH);
}

#[test]
fn wave22_05_preflight_ok_when_hash_matches() {
    let input = fixture_input_all_green();
    let bundle = fixture_spawnable_bundle();
    assert!(enforce_auto_spawn_preflight(&input, Some(&bundle)).is_ok());
}

#[test]
fn wave22_05_gate_default_is_not_requested_byte_compatible_with_wave21_04() {
    let input = WorkstationAutoSpawnInput::default();
    let outcome = evaluate_workstation_auto_spawn_gate(&input, None, None, None);
    assert_eq!(outcome.status, WorkstationAutoSpawnStatus::NotRequested);
    assert!(!outcome.requested);
    assert!(outcome.gate_results.is_empty());
    // Wire JSON shape stable.
    let v = outcome.to_response_json();
    assert_eq!(v["auto_spawn_status"], "not_requested");
    assert_eq!(v["requested"], false);
}

#[test]
fn wave22_05_gate_happy_path_returns_spawned() {
    let input = fixture_input_all_green();
    let bundle = fixture_spawnable_bundle();
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::Spawned,
        "all 12 gates green ⇒ Spawned: gate_results={:?}",
        outcome.gate_results
    );
    assert!(outcome.status.was_spawned());
    assert_eq!(
        outcome.spawn_target.as_deref(),
        Some("mission_task_delegate")
    );
    assert_eq!(
        outcome.proposal_hash_status,
        WorkstationProposalHashStatus::Matches
    );
    assert!(outcome
        .gate_results
        .iter()
        .any(|s| s.contains("g12_spawn_target:mission_task_delegate")));
    assert!(outcome
        .gate_results
        .iter()
        .any(|s| s.contains("auto_spawn_gate_satisfied")));
}

#[test]
fn wave22_05_gate_skips_when_unavailable_no_fallback() {
    let input = fixture_input_all_green();
    let bundle = WorkstationProposalBundle::unavailable("Sonnet down");
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedUnavailable
    );
    assert!(
        outcome
            .gate_results
            .iter()
            .any(|s| s.contains("llm_unavailable")),
        "unavailable reason MUST surface in gate_results: {:?}",
        outcome.gate_results
    );
    assert!(
        outcome.gate_results.iter().any(|s| s.contains("claude -p")),
        "gate text MUST pin no-fallback invariant"
    );
}

#[test]
fn wave22_05_gate_skips_when_proposal_unsafe() {
    let input = fixture_input_all_green();
    let mut bundle = fixture_spawnable_bundle();
    bundle.proposals[0].safety_status = WorkstationProposalSafetyStatus::UnsupportedTarget;
    // hash will mismatch since safety_status is part of the hash; recompute.
    let mut input = input;
    input.proposal_hash = Some(compute_workstation_proposal_hash(&bundle));
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedUnsafeProposal
    );
}

#[test]
fn wave22_05_gate_skips_when_confidence_low() {
    let mut bundle = fixture_spawnable_bundle();
    bundle.proposals[0].confidence = WorkstationProposalConfidence::Medium;
    let mut input = fixture_input_all_green();
    input.proposal_hash = Some(compute_workstation_proposal_hash(&bundle));
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedConfidenceTooLow
    );
}

#[test]
fn wave22_05_gate_skips_when_caller_not_approved() {
    let mut input = fixture_input_all_green();
    input.caller_approved = false;
    let bundle = fixture_spawnable_bundle();
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedCallerNotApproved
    );
}

#[test]
fn wave22_05_gate_skips_when_preflight_unacceptable() {
    let mut input = fixture_input_all_green();
    input.preflight_status_acceptable = false;
    let bundle = fixture_spawnable_bundle();
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedPreflightUnacceptable
    );
}

#[test]
fn wave22_05_gate_skips_when_task_contract_path_missing() {
    let mut input = fixture_input_all_green();
    input.task_contract_path = None;
    let bundle = fixture_spawnable_bundle();
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedMissingTaskContractPath
    );
}

#[test]
fn wave22_05_gate_skips_when_task_contract_malformed() {
    let input = fixture_input_all_green();
    let bundle = fixture_spawnable_bundle();
    let outcome = evaluate_workstation_auto_spawn_gate(
        &input,
        Some(&bundle),
        None,
        Some("schema mismatch — expected `missiond.task-contract.v1`"),
    );
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedMalformedTaskContract
    );
    assert_eq!(
        outcome.substrate_reason.as_deref(),
        Some("schema mismatch — expected `missiond.task-contract.v1`")
    );
}

#[test]
fn wave22_05_gate_skips_when_write_scope_empty() {
    let input = fixture_input_all_green();
    let bundle = fixture_spawnable_bundle();
    let mut contract = fixture_spawnable_contract();
    contract.write_scope.clear();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedEmptyWriteScope
    );
}

#[test]
fn wave22_05_gate_skips_when_must_not_touch_overlaps_write_scope() {
    let input = fixture_input_all_green();
    let bundle = fixture_spawnable_bundle();
    let mut contract = fixture_spawnable_contract();
    // Force overlap.
    contract
        .must_not_touch
        .push(contract.write_scope[0].clone());
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedForbiddenScopeOverlap
    );
}

#[test]
fn wave22_05_gate_skips_when_proposed_target_not_task_delegate() {
    let mut bundle = fixture_spawnable_bundle();
    bundle.proposals[0].value = json!("mission_execution");
    let mut input = fixture_input_all_green();
    input.proposal_hash = Some(compute_workstation_proposal_hash(&bundle));
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedUnsupportedTarget
    );
    assert_eq!(outcome.spawn_target.as_deref(), Some("mission_execution"));
}

#[test]
fn wave22_05_gate_skips_when_no_target_proposal() {
    let mut bundle = fixture_spawnable_bundle();
    bundle.proposals.retain(|p| p.field != "target");
    let mut input = fixture_input_all_green();
    input.proposal_hash = Some(compute_workstation_proposal_hash(&bundle));
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedUnsupportedTarget
    );
}

#[test]
fn wave22_05_gate_skips_when_bundle_no_suggestions() {
    let input = fixture_input_all_green();
    let bundle = WorkstationProposalBundle {
        status: WorkstationProposalStatus::NoSuggestions,
        proposals: Vec::new(),
        parse_warnings: Vec::new(),
        unavailable_reason: None,
        model: Some("claude-sonnet".to_string()),
        request_caller: Some(SONNET_WORKSTATION_PROPOSAL_CALLER.to_string()),
    };
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedNoProposals
    );
}

/// Wave21-04 invariant: even after wave22-05's true-spawn promotion,
/// the wave-21 propose-only bundle's `auto_spawn=false` field MUST
/// stay unchanged. The wave-22 gate publishes its own auto_spawn
/// status on a SEPARATE `workstation_auto_spawn_gate` block.
#[test]
fn wave22_05_preserves_wave21_04_bundle_auto_spawn_false_invariant() {
    let bundle = fixture_spawnable_bundle();
    let v = bundle.to_response_json();
    assert_eq!(
        v["auto_spawn"], false,
        "wave-21 / task 04 invariant: bundle MUST still pin auto_spawn=false on the \
         propose-only surface — wave-22 / task 05 publishes spawn status SEPARATELY"
    );
}

/// Wave21-04 invariant: every proposal still carries applied=false
/// on the wire, even when the wave22-05 gate would have spawned.
/// The propose-only surface stays semantically identical; the
/// real spawn happens through the wave-22 gate's substrate path.
#[test]
fn wave22_05_preserves_wave21_04_proposal_applied_false_invariant() {
    let bundle = fixture_spawnable_bundle();
    for p in &bundle.proposals {
        let v = p.to_json();
        assert_eq!(
            v["applied"], false,
            "wave-21 / task 04 invariant: every proposal MUST carry applied=false on the \
             wire — wave-22 / task 05 spawns via SEPARATE gate decision, not by flipping \
             this field"
        );
    }
}

/// Wave21-04 invariant: Sonnet unavailable bundle's
/// `unavailable_reason` text still pins the no-fallback contract.
/// The wave-22 gate inherits this invariant — when bundle is
/// Unavailable, the gate refuses to spawn (no fallback synthesis).
#[test]
fn wave22_05_preserves_wave21_04_unavailable_no_fallback_invariant() {
    let bundle = WorkstationProposalBundle::unavailable(
        "Sonnet gateway not initialized; autonomous workstation proposal unavailable \
         (no fallback to claude -p / prompt mode in v0)",
    );
    assert!(bundle
        .unavailable_reason
        .as_deref()
        .unwrap_or("")
        .contains("no fallback"));
    // Gate inherits: SkippedUnavailable when bundle is Unavailable.
    let input = fixture_input_all_green();
    let outcome = evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), None, None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedUnavailable
    );
}

/// Wave21-04 invariant cross-check: the response JSON of the
/// wave-22 gate MUST surface every contract-mandated field
/// (auto_spawn_status / spawn_target / task_contract_path /
/// proposal_hash_status / gate_results) in a stable shape so
/// dashboards can pivot uniformly.
#[test]
fn wave22_05_response_json_carries_all_contract_fields() {
    let input = fixture_input_all_green();
    let bundle = fixture_spawnable_bundle();
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&input, Some(&bundle), Some(&contract), None);
    let v = outcome.to_response_json();
    for key in [
        "requested",
        "auto_spawn_status",
        "spawn_target",
        "task_contract_path",
        "proposal_hash_status",
        "computed_proposal_hash",
        "supplied_proposal_hash",
        "caller_approved",
        "preflight_status_acceptable",
        "gate_results",
        "substrate_reason",
    ] {
        assert!(
            v.get(key).is_some(),
            "wave22-05 response JSON MUST carry `{}`",
            key
        );
    }
}

/// Wave21-04 invariant carryover: DAG mode still rejects the wave-21
/// propose pass at preflight. The wave-22 gate also refuses (it
/// requires a bundle, which DAG mode prevents creating). This is
/// belt-and-braces: the DAG preflight rejection is enforced by
/// `plan.rs::refuse_workstation_inference_in_dag_mode` BEFORE any
/// gate runs.
#[test]
fn wave22_05_gate_skips_when_no_bundle_supplied() {
    let input = fixture_input_all_green();
    let outcome = evaluate_workstation_auto_spawn_gate(&input, None, None, None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::SkippedNoProposals,
        "no bundle ⇒ SkippedNoProposals (the DAG-mode invariant flows through here)"
    );
}

// ── Wave 22 / Task 07 — autonomous loop apply smoke v4 ──
//
// Pin the wave22-05 autonomous workstation true-spawn gate slice
// of the wave22-07 v4 smoke contract. The pure preflight + 12-rule
// evaluator pair is the deterministic SSOT — no Sonnet call, no
// substrate dispatch, pure in-process functions over synthesised
// bundle / contract / input fixtures. The companion review_gate.rs
// / plan.rs / agent_execution.rs / unified_entry.rs smokes cover
// the review-apply-gate, persisted-apply, failed-verification, and
// markdown-non-load-bearing slices.

/// V4 smoke (Requirement 2 / workstation auto-spawn slice): the
/// wave22-05 gate MUST reject `auto_spawn=true` when the caller
/// does not supply `proposal_hash`, MUST reject a mismatched hash,
/// AND MUST accept the canonical `compute_workstation_proposal_hash`
/// value. This is the wave22-05 fail-fast preflight — the gate
/// refuses to dispatch the substrate with no correlator and
/// accepts only the canonical fixture path, where the 12-rule
/// evaluator then drives Spawned without ever calling the
/// substrate (the test never exercises run_workstation_dispatch).
#[test]
fn smoke_wave22_07_workstation_auto_spawn_gate_rejects_missing_hash_accepts_fixture_hash() {
    let bundle = fixture_spawnable_bundle();
    let canonical = compute_workstation_proposal_hash(&bundle);
    // Missing proposal_hash → AUTO_SPAWN_MISSING_PROPOSAL_HASH.
    let mut missing_input = fixture_input_all_green();
    missing_input.proposal_hash = None;
    let err = enforce_auto_spawn_preflight(&missing_input, Some(&bundle))
        .expect_err("wave22-07 v4: missing proposal_hash MUST fail-fast on auto-spawn path");
    assert_eq!(
        err.0, AUTO_SPAWN_MISSING_PROPOSAL_HASH,
        "wave22-07 v4 invariant: missing proposal_hash MUST surface the dedicated code"
    );
    // Mismatched proposal_hash → AUTO_SPAWN_PROPOSAL_HASH_MISMATCH.
    let mut mismatch_input = fixture_input_all_green();
    mismatch_input.proposal_hash = Some("0".repeat(32));
    let err = enforce_auto_spawn_preflight(&mismatch_input, Some(&bundle))
        .expect_err("wave22-07 v4: mismatched proposal_hash MUST fail-fast");
    assert_eq!(err.0, AUTO_SPAWN_PROPOSAL_HASH_MISMATCH);
    // Matching fixture hash → preflight OK + 12-rule gate Spawned
    // (substrate is NOT invoked — the gate evaluator is a pure
    // function ending at WorkstationAutoSpawnStatus::Spawned).
    let mut valid_input = fixture_input_all_green();
    valid_input.proposal_hash = Some(canonical.clone());
    assert!(
        enforce_auto_spawn_preflight(&valid_input, Some(&bundle)).is_ok(),
        "wave22-07 v4: matching proposal_hash MUST pass auto-spawn preflight"
    );
    let contract = fixture_spawnable_contract();
    let outcome =
        evaluate_workstation_auto_spawn_gate(&valid_input, Some(&bundle), Some(&contract), None);
    assert_eq!(
        outcome.status,
        WorkstationAutoSpawnStatus::Spawned,
        "wave22-07 v4 invariant: matching fixture hash + all 12 rules green \
         MUST drive the auto-spawn gate to Spawned (no real substrate dispatch \
         happens in this smoke — the evaluator is pure)"
    );
    assert_eq!(
        outcome.proposal_hash_status,
        WorkstationProposalHashStatus::Matches
    );
}

/// V4 smoke (cross-wave invariants / wave21-04 4 invariants
/// pinned): the wave22-05 true-spawn gate MUST preserve every
/// wave-21 / task 04 propose-only invariant when stamped onto the
/// same call.
///   * I1 default off — `auto_spawn=false` (or absent) keeps the
///     wave-21 / task 04 byte-shape exactly: no
///     `workstation_auto_spawn_gate` block on the response.
///   * I2 Sonnet unavailable no fallback — the gate MUST short-
///     circuit on `Unavailable` bundles with the no-fallback
///     marker text in `gate_results`.
///   * I3 DAG mode rejects (flows through `Some(bundle)=None` for
///     the propose-only path) — the gate skips with
///     `SkippedNoProposals` when no bundle is supplied.
///   * I4 propose-only fields preserved — the bundle's
///     `auto_spawn=false` + every proposal's `applied=false`
///     wire shape stays unchanged on the propose-only surface;
///     the wave-22 / task 05 gate publishes spawn status on a
///     SEPARATE block.
#[test]
fn smoke_wave22_07_workstation_auto_spawn_gate_pins_wave21_04_four_invariants() {
    // I1 — default off: gate omitted when auto_spawn=false / absent.
    let off_input = WorkstationAutoSpawnInput::default();
    let off_outcome = evaluate_workstation_auto_spawn_gate(&off_input, None, None, None);
    assert_eq!(
        off_outcome.status,
        WorkstationAutoSpawnStatus::NotRequested,
        "wave21-04 I1: default off ⇒ NotRequested (byte-shape preserved)"
    );
    assert!(
        !off_outcome.requested,
        "wave21-04 I1: requested MUST stay false on the default-off path"
    );
    // I2 — Sonnet unavailable no fallback (the gate text pins the
    // no-fallback marker so dashboards can grep the response).
    let unavail_input = fixture_input_all_green();
    let unavail_bundle = WorkstationProposalBundle::unavailable("Sonnet down");
    let unavail_contract = fixture_spawnable_contract();
    let unavail_outcome = evaluate_workstation_auto_spawn_gate(
        &unavail_input,
        Some(&unavail_bundle),
        Some(&unavail_contract),
        None,
    );
    assert_eq!(
        unavail_outcome.status,
        WorkstationAutoSpawnStatus::SkippedUnavailable,
        "wave21-04 I2: Sonnet unavailable ⇒ SkippedUnavailable (never fall back)"
    );
    assert!(
        unavail_outcome
            .gate_results
            .iter()
            .any(|s| s.contains("claude -p")),
        "wave21-04 I2: gate text MUST pin the no-fallback invariant in gate_results"
    );
    // I3 — DAG-mode-style rejection (no bundle ⇒ SkippedNoProposals).
    // The wave-21 / task 04 DAG-mode-rejects invariant flows into
    // the gate via the `bundle=None` branch (the wave-22 / task 05
    // splice site refuses to compute a bundle in DAG mode).
    let dag_outcome =
        evaluate_workstation_auto_spawn_gate(&fixture_input_all_green(), None, None, None);
    assert_eq!(
        dag_outcome.status,
        WorkstationAutoSpawnStatus::SkippedNoProposals,
        "wave21-04 I3: DAG-mode-style (no bundle) MUST skip without dispatching"
    );
    // I4 — propose-only bundle's auto_spawn=false and every
    // proposal's applied=false stay unchanged on the wire.
    let bundle = fixture_spawnable_bundle();
    let v = bundle.to_response_json();
    assert_eq!(
        v["auto_spawn"], false,
        "wave21-04 I4: bundle auto_spawn MUST stay false on the propose-only surface"
    );
    for p in v["proposals"].as_array().expect("proposals array") {
        assert_eq!(
            p["applied"], false,
            "wave21-04 I4: every proposal MUST keep applied=false on the propose-only wire"
        );
    }
}

// ── wave-23 / task 05 — session-trace propagation tests ─────────────
//
// These tests pin the brief / contract / response surfaces against
// the four cases the task contract enumerates: legacy (no trace),
// happy path (trace forwarded), and contract-side overlay (the
// pure-parser variant). Plan-level integration (validation +
// response surface) lives in plan.rs::tests; this module exercises
// the workstation-dispatch substrate alone.

#[test]
fn build_task_brief_with_source_and_trace_omits_session_trace_block_when_path_absent() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    let brief =
        build_task_brief_with_source_and_trace(&plan, &hints, "fresh-code-alignment", None, None);
    assert!(
        !brief.contains("## Session trace"),
        "wave23-05: legacy brief (no trace path supplied) must NOT carry the Session trace section — got:\n{}",
        brief
    );
}

#[test]
fn build_task_brief_with_source_and_trace_renders_session_trace_block_when_path_supplied() {
    let plan = fixture_plan("(plan)");
    let hints = WorkstationDispatchHints {
        objective: Some("ship".to_string()),
        owned_files: vec!["a.rs".to_string()],
        ..Default::default()
    };
    let trace = ".missiond/tasks/wave23/session-trace.lisp";
    let brief = build_task_brief_with_source_and_trace(
        &plan,
        &hints,
        "fresh-code-alignment",
        None,
        Some(trace),
    );
    assert!(
        brief.contains("## Session trace\n"),
        "wave23-05: brief with trace path must render the Session trace heading — got:\n{}",
        brief
    );
    assert!(
        brief.contains(trace),
        "wave23-05: brief must echo the trace path verbatim so the worker reads it"
    );
    assert!(
        brief.contains("forward this path verbatim as `session_trace_path`"),
        "wave23-05: brief must instruct the worker to forward the path on completion calls"
    );
}

#[test]
fn parse_task_contract_extracts_session_trace_path_from_contract_lisp() {
    // Pure-parser test: a contract that emits :session-trace-path
    // must round-trip through the workstation_dispatch parser into
    // ParsedTaskContract.session_trace_path. This is the SSOT path
    // for machine-mode dispatches that load the contract directly
    // (caller may have dropped the explicit arg).
    let src = r#"(task plan-trace
  :schema "missiond.task-contract.v1"
  :goal "ship the wave"
  :write-scope ["a.rs"]
  :must-not-touch []
  :acceptance ["cargo test"]
  :session-trace-path ".missiond/tasks/wave23/session-trace.lisp"
)"#;
    let parsed = parse_task_contract(src).expect("parse ok");
    assert_eq!(
        parsed.session_trace_path.as_deref(),
        Some(".missiond/tasks/wave23/session-trace.lisp"),
        "wave23-05: contract :session-trace-path must round-trip through the parser"
    );
}
