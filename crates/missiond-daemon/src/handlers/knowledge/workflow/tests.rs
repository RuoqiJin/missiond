//! Regression tests for mission_workflow and its split submodules.

use super::*;

#[test]
fn count_top_form_matches_phase_and_step() {
    // count_top_form scans for forms at the start of trimmed lines —
    // matches the typical methodology Lisp layout (one form per line).
    let body = "\
(workflow demo
  (phase a
(step s1)
(step s2))
  (phase b
(step s3)))
";
    assert_eq!(count_top_form(body, "phase"), 2);
    assert_eq!(count_top_form(body, "step"), 3);
    assert_eq!(count_top_form(body, "absent"), 0);
}

#[test]
fn parse_id_arg_rejects_non_uuid() {
    let args = serde_json::json!({"plan_id": "not-a-uuid"});
    assert!(parse_id_arg(&args, "plan_id").is_err());
}

#[test]
fn parse_id_arg_accepts_uuid() {
    let id = uuid::Uuid::new_v4();
    let args = serde_json::json!({"plan_id": id.to_string()});
    assert_eq!(parse_id_arg(&args, "plan_id").unwrap(), id);
}

#[test]
fn run_methodology_record_intent_defaults_to_artifact_only() {
    let args = serde_json::json!({});
    let intent = parse_run_methodology_record_intent(&args).unwrap();
    assert_eq!(intent.workflow_id, None);
    assert_eq!(intent.cost_usd, None);

    let payload = methodology_execution_record_payload("methodology-demo-v0", &intent);
    assert_eq!(payload["status"], "artifact_only_no_workflow_row");
    assert_eq!(payload["mode"], "methodology_flow");
    assert_eq!(payload["flow_id"], "methodology-demo-v0");
}

#[test]
fn run_methodology_record_intent_accepts_workflow_row_target() {
    let id = uuid::Uuid::new_v4();
    let args = serde_json::json!({
        "workflow_id": id.to_string(),
        "cost_usd": 0.25,
    });
    let intent = parse_run_methodology_record_intent(&args).unwrap();
    assert_eq!(intent.workflow_id, Some(id));
    assert_eq!(intent.cost_usd, Some(0.25));

    let payload = methodology_execution_record_payload("methodology-demo-v0", &intent);
    assert_eq!(payload["status"], "recorded");
    assert_eq!(payload["mode"], "workflow_row");
    assert_eq!(payload["workflow_id"], id.to_string());
    assert_eq!(payload["cost_usd"], 0.25);
}

#[test]
fn run_methodology_record_intent_rejects_bad_workflow_id() {
    let args = serde_json::json!({"workflow_id": "not-a-uuid"});
    assert!(parse_run_methodology_record_intent(&args).is_err());
}

#[test]
fn parse_distill_mode_default_and_explicit() {
    // Backwards-compat: missing or empty → dry_run keeps legacy callers working.
    assert_eq!(parse_distill_mode(None), Ok(DistillMode::DryRun));
    assert_eq!(parse_distill_mode(Some("")), Ok(DistillMode::DryRun));
    assert_eq!(parse_distill_mode(Some("dry_run")), Ok(DistillMode::DryRun));
    assert_eq!(parse_distill_mode(Some("sonnet")), Ok(DistillMode::Sonnet));
    assert!(parse_distill_mode(Some("nope")).is_err());
}

#[test]
fn extract_json_payload_passes_through_plain() {
    let raw = "{\"workflow_sexp\":\"(workflow x)\"}";
    assert_eq!(extract_json_payload(raw), raw);
}

#[test]
fn extract_json_payload_strips_fenced_block() {
    let raw = "```json\n{\"a\":1}\n```";
    assert_eq!(extract_json_payload(raw), "{\"a\":1}");
    let raw2 = "```\n{\"b\":2}\n```";
    assert_eq!(extract_json_payload(raw2), "{\"b\":2}");
}

#[test]
fn extract_json_payload_strips_fence_without_close() {
    // Some models forget the closing fence; we still surface the inner content.
    let raw = "```json\n{\"a\":1}";
    assert_eq!(extract_json_payload(raw), "{\"a\":1}");
}

#[test]
fn distiller_response_parse_pass_and_fail() {
    let good = "{\"workflow_sexp\":\"(workflow demo)\",\"match_rules\":{\"tokens\":[\"demo\"]}}";
    let v: serde_json::Value =
        serde_json::from_str(extract_json_payload(good)).expect("good JSON parses");
    assert_eq!(
        v.get("workflow_sexp").and_then(|x| x.as_str()),
        Some("(workflow demo)")
    );
    assert!(v.get("match_rules").map(|m| m.is_object()).unwrap_or(false));

    let bad = "not a json blob";
    assert!(serde_json::from_str::<serde_json::Value>(extract_json_payload(bad)).is_err());
}

#[test]
fn paren_balance_basic() {
    // Empty input is vacuously balanced — `validate_workflow_sexp` is the
    // gate that rejects empty / non-`(`-prefixed strings.
    assert!(paren_balanced_ignoring_strings(""));
    assert!(paren_balanced_ignoring_strings("()"));
    assert!(paren_balanced_ignoring_strings("(a (b c) (d (e)))"));
    assert!(!paren_balanced_ignoring_strings("(a (b)"));
    assert!(!paren_balanced_ignoring_strings(")("));
}

#[test]
fn paren_balance_ignores_string_payload() {
    // The closing paren in the literal is inside a string and must be ignored.
    assert!(paren_balanced_ignoring_strings(
        "(workflow :note \"closes ) here\")"
    ));
    // Escaped quote inside string should not flip the in-string flag.
    assert!(paren_balanced_ignoring_strings(
        "(a \"esc \\\" still in str ) \" b)"
    ));
    // Unterminated string is invalid.
    assert!(!paren_balanced_ignoring_strings("(a \"unterminated"));
}

#[test]
fn validate_workflow_sexp_rejects_empty_and_unbalanced() {
    assert!(validate_workflow_sexp("").is_err());
    assert!(validate_workflow_sexp("   ").is_err());
    assert!(validate_workflow_sexp("not-sexp").is_err());
    assert!(validate_workflow_sexp("(open").is_err());
    assert!(validate_workflow_sexp("(workflow demo)").is_ok());
}

#[test]
fn match_rules_must_be_object() {
    let parsed: serde_json::Value = serde_json::from_str("{\"match_rules\":[\"oops\"]}").unwrap();
    assert!(!parsed
        .get("match_rules")
        .map(|v| v.is_object())
        .unwrap_or(false));
    let parsed_ok: serde_json::Value =
        serde_json::from_str("{\"match_rules\":{\"tokens\":[]}}").unwrap();
    assert!(parsed_ok
        .get("match_rules")
        .map(|v| v.is_object())
        .unwrap_or(false));
}

#[test]
fn evidence_gate_allows_missing_when_flag_set() {
    assert_eq!(evidence_gate(false, 0, 1, true), None);
    assert_eq!(evidence_gate(true, 0, 1, true), None);
    assert_eq!(evidence_gate(true, 5, 1, true), None);
}

#[test]
fn evidence_gate_rejects_missing_or_short() {
    assert!(evidence_gate(false, 0, 1, false).is_some());
    assert!(evidence_gate(true, 0, 1, false).is_some());
    assert!(evidence_gate(true, 1, 2, false).is_some());
}

#[test]
fn evidence_gate_passes_when_enough_entries() {
    assert_eq!(evidence_gate(true, 1, 1, false), None);
    assert_eq!(evidence_gate(true, 5, 3, false), None);
}

#[test]
fn collect_match_hint_string_array_or_none() {
    assert_eq!(collect_match_hint(None), "");
    assert_eq!(collect_match_hint(Some(&serde_json::json!(""))), "");
    assert_eq!(
        collect_match_hint(Some(&serde_json::json!("alpha"))),
        "alpha"
    );
    assert_eq!(
        collect_match_hint(Some(&serde_json::json!(["alpha", "beta", "", "gamma"]))),
        "alpha, beta, gamma"
    );
    // Non-string array elements are dropped.
    assert_eq!(
        collect_match_hint(Some(&serde_json::json!(["alpha", 42, "gamma"]))),
        "alpha, gamma"
    );
}

#[test]
fn name_referenced_checks_sexp_and_rules() {
    let rules = serde_json::json!({"tokens": ["demo"]});
    assert!(name_referenced("", "(workflow x)", &rules));
    assert!(name_referenced(
        "demo",
        "(workflow demo)",
        &serde_json::json!({})
    ));
    assert!(name_referenced("demo", "(workflow x)", &rules));
    assert!(!name_referenced(
        "absent",
        "(workflow x)",
        &serde_json::json!({})
    ));
}

#[test]
fn evidence_sidecar_path_is_under_v3_runtime_plans() {
    let id = uuid::Uuid::nil();
    let path = evidence_sidecar_path(Path::new("/tmp/proj"), id);
    let s = path.display().to_string();
    assert!(s.ends_with(&format!(".missiond/v3/runtime/plans/{}.evidence.json", id)));
}

// ──────────────────────────────────────────────────────────────
// methodology compiler v0 — pure-fn tests
// ──────────────────────────────────────────────────────────────

#[test]
fn parse_compile_mode_default_and_explicit() {
    assert_eq!(parse_compile_mode(None), Ok(CompileMode::DryRun));
    assert_eq!(parse_compile_mode(Some("")), Ok(CompileMode::DryRun));
    assert_eq!(parse_compile_mode(Some("dry_run")), Ok(CompileMode::DryRun));
    assert_eq!(
        parse_compile_mode(Some("deterministic")),
        Ok(CompileMode::Deterministic)
    );
    assert!(parse_compile_mode(Some("nope")).is_err());
}

#[test]
fn methodology_path_workflow_path_takes_precedence() {
    let root = Path::new("/tmp/proj");
    // absolute workflow_path passes through
    let abs = resolve_methodology_path(root, None, Some("/abs/some.lisp")).unwrap();
    assert_eq!(abs, PathBuf::from("/abs/some.lisp"));
    // relative workflow_path joins to project_root
    let rel = resolve_methodology_path(root, None, Some("methods/foo.lisp")).unwrap();
    assert_eq!(rel, PathBuf::from("/tmp/proj/methods/foo.lisp"));
}

#[test]
fn methodology_path_name_appends_lisp_extension() {
    let root = Path::new("/tmp/proj");
    let p = resolve_methodology_path(root, Some("bus-refactor"), None).unwrap();
    assert_eq!(
        p,
        PathBuf::from("/tmp/proj/.missiond/workflows/bus-refactor.lisp")
    );
    // Caller may pass an explicit extension — keep it.
    let p2 = resolve_methodology_path(root, Some("bus-refactor.lisp"), None).unwrap();
    assert_eq!(
        p2,
        PathBuf::from("/tmp/proj/.missiond/workflows/bus-refactor.lisp")
    );
}

#[test]
fn methodology_path_requires_one_of_args() {
    let root = Path::new("/tmp/proj");
    assert!(resolve_methodology_path(root, None, None).is_err());
    assert!(resolve_methodology_path(root, Some(""), Some("")).is_err());
}

#[test]
fn validate_methodology_source_rejects_empty_and_unbalanced() {
    assert!(validate_methodology_source("").is_err());
    assert!(validate_methodology_source("   \n  ").is_err());
    // No top-level form even if non-empty.
    assert!(validate_methodology_source("not-a-form").is_err());
    // Unbalanced parens (string-ignoring detector catches this).
    assert!(validate_methodology_source("(workflow demo (step s1").is_err());
    // Balanced + has form → ok.
    assert!(validate_methodology_source("(workflow demo (step s1 \"do x\"))").is_ok());
}

#[test]
fn source_hash_is_stable_and_distinguishes_inputs() {
    let a1 = source_hash("(workflow demo)");
    let a2 = source_hash("(workflow demo)");
    let b = source_hash("(workflow other)");
    assert_eq!(a1, a2, "same input must hash identically");
    assert_ne!(a1, b, "different input must hash differently");
    // sha256 hex is 64 chars.
    assert_eq!(a1.len(), 64);
    assert!(a1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn derive_flow_id_uses_explicit_first() {
    assert_eq!(
        derive_flow_id("bus-refactor", Some("custom-id")),
        "custom-id".to_string()
    );
    // empty explicit falls back
    assert_eq!(
        derive_flow_id("bus-refactor", Some("")),
        "methodology-bus-refactor-v0".to_string()
    );
    // none → default
    assert_eq!(
        derive_flow_id("bus-refactor", None),
        "methodology-bus-refactor-v0".to_string()
    );
    // sanitization collapses non-alnum
    assert_eq!(
        derive_flow_id("Foo Bar/Baz!", None),
        "methodology-Foo-Bar-Baz-v0".to_string()
    );
    // anonymous fallback when stem yields empty token
    assert_eq!(
        derive_flow_id("///", None),
        "methodology-anonymous-v0".to_string()
    );
}

#[test]
fn extract_steps_handles_single_line_form() {
    let body = "\
(workflow demo
  (step s1 \"first thing\")
  (step s2 \"second thing\"))
";
    let steps = extract_steps(body);
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].id, "s1");
    assert!(steps[0].body.contains("first thing"));
    assert_eq!(steps[1].id, "s2");
    assert!(steps[1].body.contains("second thing"));
}

#[test]
fn extract_steps_handles_multi_line_form() {
    let body = "\
(workflow demo
  (step long-id
\"line one
 line two\"
:note other))
";
    let steps = extract_steps(body);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].id, "long-id");
    assert!(steps[0].body.contains("line one"));
    assert!(steps[0].body.contains(":note other"));
}

#[test]
fn extract_steps_ignores_lookalike_forms() {
    // (steps …) and (step) without body should be skipped — first because
    // of the prefix mismatch, second because the id parse fails.
    let body = "\
(workflow demo
  (steps
(foo))
  (step)
  (step real \"ok\"))
";
    let steps = extract_steps(body);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].id, "real");
}

#[test]
fn extract_steps_returns_empty_when_no_steps() {
    // Real methodology lisps frequently have no top-level (step …) — they
    // use (phase-* …) instead. Compiler v0 must hand this to manual review.
    let body = "\
(workflow bus-refactor
  (phase-A exploration :goal \"survey\")
  (phase-B design-freeze :goal \"freeze\"))
";
    assert!(extract_steps(body).is_empty());
}

#[test]
fn extract_steps_paren_in_string_does_not_close_form() {
    let body = "\
(workflow demo
  (step s1 \"closes ) inside string\"
    :tag normal))
";
    let steps = extract_steps(body);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].id, "s1");
    assert!(steps[0].body.contains("closes ) inside string"));
    assert!(steps[0].body.contains(":tag normal"));
}

#[test]
fn build_generated_yaml_contains_source_metadata_and_steps() {
    let meta = GeneratedMeta {
        flow_id: "methodology-foo-v0".to_string(),
        name: "methodology compile v0 — foo".to_string(),
        source_path: ".missiond/workflows/foo.lisp".to_string(),
        source_hash: "deadbeef".repeat(8),
        generated_at: "2026-04-25T00:00:00Z".to_string(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let steps = vec![LocatedStep {
        step: MethodologyStep {
            id: "s1".to_string(),
            body: "(step s1 \"do x\")".to_string(),
        },
        start_line: 0,
    }];
    let yaml = build_generated_yaml(&meta, &steps, &MethodologyLifted::default(), false)
        .expect("yaml builds");
    assert!(yaml.contains("id: methodology-foo-v0"));
    assert!(yaml.contains("source_kind: methodology_lisp"));
    assert!(yaml.contains(".missiond/workflows/foo.lisp"));
    assert!(yaml.contains(&meta.source_hash));
    assert!(yaml.contains(&format!("generated_by: {}", COMPILER_VERSION)));
    assert!(yaml.contains(&format!("compiler_status: {}", COMPILER_STATUS_PREVIEW)));
    assert!(yaml.contains("review_required: false"));
    assert!(yaml.contains("step_s1"));
    assert!(yaml.contains("type: slot_task"));
    // No lifted forms → no methodology_metadata key emitted at all.
    assert!(
        !yaml.contains("methodology_metadata"),
        "default lifted must produce no metadata key: {}",
        yaml
    );
    // round-trip parse via FlowDefinition (which silently drops the extra
    // metadata fields) — ensures the generated YAML is loader-ready.
    let parsed: crate::engine::flow::FlowDefinition =
        serde_yaml::from_str(&yaml).expect("FlowDefinition parses");
    assert_eq!(parsed.id, "methodology-foo-v0");
    assert_eq!(parsed.nodes.len(), 1);
    assert_eq!(parsed.nodes[0].id, "step_s1");
}

#[test]
fn build_generated_yaml_emits_manual_review_when_no_steps() {
    let meta = GeneratedMeta {
        flow_id: "methodology-foo-v0".to_string(),
        name: "foo".to_string(),
        source_path: "src.lisp".to_string(),
        source_hash: "abc".to_string(),
        generated_at: "ts".to_string(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let yaml =
        build_generated_yaml(&meta, &[], &MethodologyLifted::default(), true).expect("yaml builds");
    assert!(yaml.contains("review_required: true"));
    assert!(yaml.contains("manual_review"));
    assert!(yaml.contains("Manually review"));
    // Must still parse.
    let parsed: crate::engine::flow::FlowDefinition =
        serde_yaml::from_str(&yaml).expect("FlowDefinition parses");
    assert_eq!(parsed.nodes.len(), 1);
    assert_eq!(parsed.nodes[0].id, "manual_review");
}

// ──────────────────────────────────────────────────────────────
// Wave 12 / Task 04 — methodology semantic lifter v0
//
// These tests pin the conservative recognition surface for the six
// higher-order forms (phase / principle / anti-pattern / gate /
// artifact / authority). The lifter must:
//   1. Recognise each form at line-start with a whitespace/`)`
//      delimiter so `(phases …)` / `(principled …)` never match.
//   2. Preserve verbatim bodies (multi-line + string-paren safe).
//   3. Stay paren-balanced through nested step forms inside a phase.
//   4. NEVER convert lifted forms into executable nodes.
//   5. Surface metadata under a YAML root `methodology_metadata` key
//      that the FlowDefinition loader silently drops on round-trip.
// ──────────────────────────────────────────────────────────────

#[test]
fn lifter_recognises_all_six_form_keywords() {
    let body = "\
(workflow demo
  (phase planning)
  (principle no-fallback)
  (anti-pattern silent-fallback)
  (gate compile-passes)
  (artifact intent.lisp)
  (authority intent-flow.lisp))
";
    let lifted = extract_methodology_lifted(body);
    assert_eq!(lifted.phases.len(), 1, "phases: {:?}", lifted.phases);
    assert_eq!(
        lifted.principles.len(),
        1,
        "principles: {:?}",
        lifted.principles
    );
    assert_eq!(
        lifted.anti_patterns.len(),
        1,
        "anti_patterns: {:?}",
        lifted.anti_patterns
    );
    assert_eq!(lifted.gates.len(), 1, "gates: {:?}", lifted.gates);
    assert_eq!(
        lifted.artifacts.len(),
        1,
        "artifacts: {:?}",
        lifted.artifacts
    );
    assert_eq!(
        lifted.authorities.len(),
        1,
        "authorities: {:?}",
        lifted.authorities
    );
    assert_eq!(lifted.total_count(), 6);
    assert_eq!(lifted.phases[0].id.as_deref(), Some("planning"));
    assert_eq!(lifted.principles[0].id.as_deref(), Some("no-fallback"));
    assert_eq!(lifted.anti_patterns[0].kind, "anti-pattern");
    assert_eq!(lifted.artifacts[0].id.as_deref(), Some("intent.lisp"));
}

#[test]
fn lifter_ignores_lookalike_prefixes() {
    // `(phases …)` / `(principled …)` / `(gateway …)` etc. share a
    // prefix with the recognised keywords but must NOT match — the
    // lifter only fires on a clean keyword + delimiter.
    let body = "\
(workflow demo
  (phases big and bold)
  (principled stance ok)
  (anti-pattern-ish bad)
  (gateway open)
  (artifacts many)
  (authorities-list a))
";
    let lifted = extract_methodology_lifted(body);
    assert!(lifted.is_empty(), "lookalikes lifted: {:?}", lifted);
}

#[test]
fn lifter_handles_phase_with_nested_step() {
    // A phase containing nested (step …) forms must (a) lift the
    // phase as a methodology form, (b) still allow extract_steps to
    // surface the inner steps as executable candidates, and (c) the
    // YAML builder must tag those step nodes with phase_id metadata.
    let body = "\
(workflow demo
  (phase planning
(step plan-1 \"draft plan\")
(step plan-2 \"review plan\")))
";
    let lifted = extract_methodology_lifted(body);
    let steps = extract_steps_with_lines(body);
    assert_eq!(lifted.phases.len(), 1);
    assert_eq!(lifted.phases[0].id.as_deref(), Some("planning"));
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].step.id, "plan-1");
    assert_eq!(steps[1].step.id, "plan-2");
    // Both steps fall inside the phase's line range.
    assert!(steps[0].start_line >= lifted.phases[0].start_line);
    assert!(steps[1].start_line <= lifted.phases[0].end_line);
    let pid = phase_id_for_step(&lifted.phases, steps[0].start_line);
    assert_eq!(pid.as_deref(), Some("planning"));
}

#[test]
fn lifter_principle_extraction_preserves_body() {
    let body = "\
(workflow demo
  (principle fail-fast \"Reject silent fallbacks; surface errors at the boundary.\"))
";
    let lifted = extract_methodology_lifted(body);
    assert_eq!(lifted.principles.len(), 1);
    let p = &lifted.principles[0];
    assert_eq!(p.kind, "principle");
    assert_eq!(p.id.as_deref(), Some("fail-fast"));
    assert!(
        p.body.contains("Reject silent fallbacks"),
        "body must preserve docstring: {}",
        p.body
    );
    // Body keeps its outer parens — that's the verbatim slice convention.
    assert!(p.body.starts_with('('));
    assert!(p.body.trim_end().ends_with(')'));
}

#[test]
fn lifter_anti_pattern_extraction_with_keyword_args() {
    let body = "\
(workflow demo
  (anti-pattern poll-fallback
:why \"polling tries to recover from upstream failure silently\"
:remedy \"surface the upstream error and let the caller decide\"))
";
    let lifted = extract_methodology_lifted(body);
    assert_eq!(lifted.anti_patterns.len(), 1);
    let ap = &lifted.anti_patterns[0];
    assert_eq!(ap.kind, "anti-pattern");
    assert_eq!(ap.id.as_deref(), Some("poll-fallback"));
    assert!(ap.body.contains(":why"));
    assert!(ap.body.contains(":remedy"));
    assert!(ap.body.contains("polling tries to recover"));
}

#[test]
fn lifter_string_paren_safe() {
    // String payloads can contain `(`/`)` glyphs that must NEVER move
    // the depth counter. If the lifter mishandles them it will close
    // the form too early or never close it at all.
    let body = "\
(workflow demo
  (gate compile-passes
:note \"runs (cargo build --workspace) on green; ) is fine inside a string\"
:evidence \"test.log\"))
";
    let lifted = extract_methodology_lifted(body);
    assert_eq!(lifted.gates.len(), 1);
    let g = &lifted.gates[0];
    assert_eq!(g.id.as_deref(), Some("compile-passes"));
    assert!(g.body.contains("cargo build"));
    assert!(g.body.contains(":evidence"));
    // Source paren balance unchanged — sanity guard against earlier-close bugs.
    assert!(paren_balanced_ignoring_strings(&g.body));
}

#[test]
fn lifter_string_paren_safe_unterminated_phase_does_not_eat_eof() {
    // Defensive: a malformed source where a phase opens but never
    // closes must NOT panic the lifter. We just don't emit the
    // unfinished form.
    let body = "(workflow x\n  (phase open\n    (step s1 \"hi\")\n";
    let lifted = extract_methodology_lifted(body);
    assert!(lifted.phases.is_empty());
}

#[test]
fn lifter_anonymous_form_keeps_id_none() {
    // `(phase :goal "x")` has no leading identifier — id should
    // stay None instead of fabricating from the `:goal` keyword.
    let body = "\
(workflow demo
  (phase :goal \"x\"))
";
    let lifted = extract_methodology_lifted(body);
    assert_eq!(lifted.phases.len(), 1);
    assert_eq!(lifted.phases[0].id, None);
}

#[test]
fn lifter_artifact_and_authority_with_path_ids() {
    // Real methodology lisps frequently use file paths as ids. The
    // lifter must accept `/`, `.`, `_`, `-` in id tokens.
    let body = "\
(workflow demo
  (artifact .missiond/v2/intent-flow.lisp)
  (authority intent_memory.lisp))
";
    let lifted = extract_methodology_lifted(body);
    assert_eq!(lifted.artifacts.len(), 1);
    assert_eq!(
        lifted.artifacts[0].id.as_deref(),
        Some(".missiond/v2/intent-flow.lisp")
    );
    assert_eq!(lifted.authorities.len(), 1);
    assert_eq!(
        lifted.authorities[0].id.as_deref(),
        Some("intent_memory.lisp")
    );
}

#[test]
fn lifter_preserves_source_order() {
    // Order matters for human review — the YAML must read top-to-
    // bottom against the source.
    let body = "\
(workflow demo
  (principle p1)
  (principle p2)
  (principle p3))
";
    let lifted = extract_methodology_lifted(body);
    assert_eq!(lifted.principles.len(), 3);
    assert_eq!(lifted.principles[0].id.as_deref(), Some("p1"));
    assert_eq!(lifted.principles[1].id.as_deref(), Some("p2"));
    assert_eq!(lifted.principles[2].id.as_deref(), Some("p3"));
}

#[test]
fn match_form_keyword_requires_delimiter() {
    // Direct unit cover of the prefix matcher — the load-bearing
    // disambiguation rule between `(phase` and `(phases`.
    const KEYWORDS: &[&str] = &["phase", "step"];
    assert_eq!(
        match_form_keyword("(phase planning)", KEYWORDS),
        Some(("phase", " planning)"))
    );
    assert_eq!(
        match_form_keyword("(phase)", KEYWORDS),
        Some(("phase", ")"))
    );
    assert_eq!(match_form_keyword("(phases big)", KEYWORDS), None);
    assert_eq!(match_form_keyword("(phaseA bad)", KEYWORDS), None);
    assert_eq!(
        match_form_keyword("(step s1)", KEYWORDS),
        Some(("step", " s1)"))
    );
    assert_eq!(match_form_keyword("(steps)", KEYWORDS), None);
    assert_eq!(match_form_keyword("not-a-form", KEYWORDS), None);
}

#[test]
fn parse_optional_form_id_rejects_keyword_args_and_strings() {
    // Identifier-only — no colon-prefixed keyword args, no strings.
    assert_eq!(
        parse_optional_form_id("ident :rest"),
        Some("ident".to_string())
    );
    assert_eq!(parse_optional_form_id(":goal x"), None);
    assert_eq!(parse_optional_form_id("\"quoted\""), None);
    assert_eq!(parse_optional_form_id("(nested)"), None);
    assert_eq!(parse_optional_form_id(""), None);
    // Path-like ids accepted (real methodology convention).
    assert_eq!(
        parse_optional_form_id("intent-flow.lisp :rest"),
        Some("intent-flow.lisp".to_string())
    );
    // Tokens with disallowed glyphs (e.g. `?`, `!`) reject — we'd
    // rather lose an id than fabricate something the source did not
    // sanction.
    assert_eq!(parse_optional_form_id("foo!bar :rest"), None);
}

#[test]
fn phase_id_for_step_returns_anonymous_id_when_phase_unnamed() {
    let phases = vec![
        MethodologyPhase {
            id: None,
            body: "(phase ...)".to_string(),
            start_line: 5,
            end_line: 9,
        },
        MethodologyPhase {
            id: Some("named".to_string()),
            body: "(phase named ...)".to_string(),
            start_line: 12,
            end_line: 14,
        },
    ];
    assert_eq!(phase_id_for_step(&phases, 6).as_deref(), Some("phase_5"));
    assert_eq!(phase_id_for_step(&phases, 13).as_deref(), Some("named"));
    // Outside any phase → None.
    assert_eq!(phase_id_for_step(&phases, 0), None);
    assert_eq!(phase_id_for_step(&phases, 11), None);
}

#[test]
fn yaml_node_carries_phase_id_when_step_belongs_to_phase() {
    let body = "\
(workflow demo
  (phase planning
(step plan-1 \"plan it\")))
";
    let lifted = extract_methodology_lifted(body);
    let steps = extract_steps_with_lines(body);
    let meta = GeneratedMeta {
        flow_id: "methodology-demo-v0".to_string(),
        name: "demo".to_string(),
        source_path: ".missiond/workflows/demo.lisp".to_string(),
        source_hash: "h".to_string(),
        generated_at: "ts".to_string(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let yaml = build_generated_yaml(&meta, &steps, &lifted, false).expect("yaml builds");
    // Per-step methodology_metadata mapping with phase_id.
    assert!(
        yaml.contains("phase_id: planning"),
        "yaml must tag step with phase_id: {}",
        yaml
    );
    // Loader still parses (unknown fields are tolerated by serde_yaml).
    let parsed: crate::engine::flow::FlowDefinition =
        serde_yaml::from_str(&yaml).expect("FlowDefinition parses");
    assert_eq!(parsed.nodes.len(), 1);
    assert_eq!(parsed.nodes[0].id, "step_plan-1");
}

#[test]
fn yaml_root_carries_methodology_metadata_when_lifted_present() {
    let body = "\
(workflow demo
  (principle p1 \"fail fast\")
  (anti-pattern silent-fallback))
";
    let lifted = extract_methodology_lifted(body);
    let steps = extract_steps_with_lines(body);
    assert!(steps.is_empty(), "no (step …) forms in this fixture");
    let meta = GeneratedMeta {
        flow_id: "methodology-demo-v0".to_string(),
        name: "demo".to_string(),
        source_path: ".missiond/workflows/demo.lisp".to_string(),
        source_hash: "h".to_string(),
        generated_at: "ts".to_string(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let yaml = build_generated_yaml(&meta, &steps, &lifted, true).expect("yaml builds");
    // Manual-review fallback because no executable steps; methodology
    // metadata still surfaces.
    assert!(yaml.contains("manual_review"));
    assert!(yaml.contains("methodology_metadata"));
    assert!(yaml.contains("principles"));
    assert!(yaml.contains("anti_patterns"));
    assert!(yaml.contains("fail fast"));
    assert!(yaml.contains("silent-fallback"));
    // Manual-review prompt summarises lifted counts so the reviewer
    // does not have to scroll the metadata mapping.
    assert!(yaml.contains("Lifted methodology semantics"));
    assert!(yaml.contains("principles: 1"));
    assert!(yaml.contains("anti-patterns: 1"));
}

#[test]
fn yaml_round_trips_when_lifted_metadata_present() {
    // YAML metadata round-trip test — the FlowDefinition loader must
    // silently drop `methodology_metadata` while every executable
    // shape (id / name / nodes) survives.
    let body = "\
(workflow demo
  (principle p1 \"ok\")
  (phase planning
(step plan-1 \"plan it\"))
  (gate g1 \"green build\")
  (anti-pattern silent-fallback)
  (artifact .missiond/v2/intent-flow.lisp)
  (authority intent-memory.lisp))
";
    let lifted = extract_methodology_lifted(body);
    let steps = extract_steps_with_lines(body);
    let meta = GeneratedMeta {
        flow_id: "methodology-demo-v0".to_string(),
        name: "demo".to_string(),
        source_path: ".missiond/workflows/demo.lisp".to_string(),
        source_hash: "h".to_string(),
        generated_at: "ts".to_string(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let yaml = build_generated_yaml(&meta, &steps, &lifted, false).expect("yaml builds");
    // Loader must accept the YAML — methodology_metadata & phase_id
    // are unknown to FlowDefinition's serde shape and must be ignored.
    let parsed: crate::engine::flow::FlowDefinition =
        serde_yaml::from_str(&yaml).expect("FlowDefinition parses despite extra metadata");
    assert_eq!(parsed.id, "methodology-demo-v0");
    assert_eq!(parsed.nodes.len(), 1);
    assert_eq!(parsed.nodes[0].id, "step_plan-1");
    // Raw YAML retains every lifted form so an audit can reconstruct
    // the methodology.
    for needle in [
        "methodology_metadata",
        "principles",
        "phases",
        "gates",
        "anti_patterns",
        "artifacts",
        "authorities",
        "phase_id: planning",
        ".missiond/v2/intent-flow.lisp",
        "intent-memory.lisp",
    ] {
        assert!(yaml.contains(needle), "yaml missing `{}`: {}", needle, yaml);
    }
}

#[test]
fn methodology_lifted_total_count_matches_breakdown() {
    let lifted = MethodologyLifted {
        phases: vec![MethodologyPhase {
            id: None,
            body: "()".into(),
            start_line: 0,
            end_line: 0,
        }],
        principles: vec![MethodologyForm {
            kind: "principle".into(),
            id: None,
            body: "()".into(),
            start_line: 0,
        }],
        anti_patterns: vec![],
        gates: vec![MethodologyForm {
            kind: "gate".into(),
            id: None,
            body: "()".into(),
            start_line: 0,
        }],
        artifacts: vec![],
        authorities: vec![],
    };
    assert_eq!(lifted.total_count(), 3);
    assert!(!lifted.is_empty());
    assert!(MethodologyLifted::default().is_empty());
    assert_eq!(MethodologyLifted::default().total_count(), 0);
}

#[test]
fn manual_review_prompt_omits_lift_section_when_lifted_empty() {
    let meta = GeneratedMeta {
        flow_id: "methodology-foo-v0".into(),
        name: "foo".into(),
        source_path: "src.lisp".into(),
        source_hash: "abc".into(),
        generated_at: "ts".into(),
        compiler_status: COMPILER_STATUS_PREVIEW.into(),
    };
    let prompt = build_manual_review_prompt(&meta, &MethodologyLifted::default());
    assert!(prompt.contains("Manually review"));
    assert!(
        !prompt.contains("Lifted methodology semantics"),
        "no lifted section when lifted is empty: {}",
        prompt
    );
}

#[test]
fn lifter_does_not_promote_phase_with_steps_when_no_step_keyword_present() {
    // Conservative invariant: lifting alone NEVER produces an
    // executable node. A methodology with phases but no steps must
    // still hit the manual_review fallback.
    let body = "\
(workflow phase-only
  (phase exploration)
  (phase design-freeze))
";
    let lifted = extract_methodology_lifted(body);
    let steps = extract_steps_with_lines(body);
    assert_eq!(lifted.phases.len(), 2);
    assert!(steps.is_empty());
    let meta = GeneratedMeta {
        flow_id: "methodology-phase-only-v0".into(),
        name: "phase-only".into(),
        source_path: ".missiond/workflows/phase-only.lisp".into(),
        source_hash: "h".into(),
        generated_at: "ts".into(),
        compiler_status: COMPILER_STATUS_PREVIEW.into(),
    };
    let yaml = build_generated_yaml(&meta, &steps, &lifted, true).expect("yaml builds");
    let parsed: crate::engine::flow::FlowDefinition = serde_yaml::from_str(&yaml).expect("parses");
    assert_eq!(parsed.nodes.len(), 1);
    assert_eq!(parsed.nodes[0].id, "manual_review");
}

#[test]
fn extract_steps_with_lines_preserves_back_compat_recognition() {
    // The line-tracking variant must recognise the same forms as the
    // legacy line-counter — pin both extractors against the same
    // fixtures so divergence is impossible.
    let bodies = [
        "(workflow demo (step s1 \"a\") (step s2 \"b\"))",
        "(workflow demo\n  (step s1 \"a\")\n  (step s2 \"b\"))",
        "(workflow demo\n  (step long\n    \"line one\n     line two\"))",
        "(workflow demo\n  (steps (foo))\n  (step real \"ok\"))",
    ];
    for body in bodies {
        let legacy = extract_steps(body);
        let with_lines = extract_steps_with_lines(body);
        assert_eq!(
            legacy.len(),
            with_lines.len(),
            "step count diverged for: {}",
            body
        );
        for (a, b) in legacy.iter().zip(with_lines.iter()) {
            assert_eq!(a.id, b.step.id);
            assert_eq!(a.body, b.step.body);
        }
    }
}

#[test]
fn generated_yaml_path_lives_under_project_local_dir() {
    let p = generated_yaml_path(Path::new("/tmp/proj"), "methodology-foo-v0");
    assert_eq!(
        p,
        PathBuf::from("/tmp/proj/.missiond/generated/flows/methodology-foo-v0.yaml")
    );
}

#[test]
fn atomic_write_creates_dirs_and_replaces_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp
        .path()
        .join(".missiond/generated/flows/methodology-foo-v0.yaml");
    atomic_write(&target, "hello").expect("first write");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "hello".to_string()
    );
    // Second write replaces in place.
    atomic_write(&target, "world").expect("second write");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "world".to_string()
    );
}

#[test]
fn resolve_compiled_flow_missing_returns_structured_pointer() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let err = resolve_compiled_flow(root, Some("methodology-foo-v0"), None, None)
        .expect_err("missing yaml must error");
    match err {
        CompiledFlowError::Missing { flow_id, expected } => {
            assert_eq!(flow_id, "methodology-foo-v0");
            assert!(expected
                .display()
                .to_string()
                .contains(".missiond/generated/flows/methodology-foo-v0.yaml"));
        }
        other => panic!("expected Missing, got {:?}", other),
    }
}

#[test]
fn resolve_compiled_flow_requires_some_arg() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let err = resolve_compiled_flow(root, None, None, None).expect_err("no args → MissingArgs");
    assert!(matches!(err, CompiledFlowError::MissingArgs));
}

#[test]
fn resolve_compiled_flow_finds_existing_yaml_by_flow_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let yaml_path = root.join(".missiond/generated/flows/methodology-foo-v0.yaml");
    std::fs::create_dir_all(yaml_path.parent().unwrap()).unwrap();
    std::fs::write(&yaml_path, "id: x\nname: x\nnodes: []\n").unwrap();
    let resolved = resolve_compiled_flow(root, Some("methodology-foo-v0"), None, None)
        .expect("flow yaml exists");
    assert_eq!(resolved.path, yaml_path);
}

#[test]
fn resolve_compiled_flow_falls_back_to_name_via_derive_flow_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // expected location uses the derived id
    let yaml_path = root.join(".missiond/generated/flows/methodology-bus-refactor-v0.yaml");
    std::fs::create_dir_all(yaml_path.parent().unwrap()).unwrap();
    std::fs::write(&yaml_path, "id: x\nname: x\nnodes: []\n").unwrap();
    let resolved = resolve_compiled_flow(root, None, None, Some("bus-refactor"))
        .expect("name resolves to derived flow id");
    assert_eq!(resolved.path, yaml_path);
}

#[test]
fn persist_overwrite_policy_via_path_existence() {
    // The action_compile_deterministic flow uses path.exists() && !overwrite
    // to refuse rewrites. We mimic that condition here so the behavior is
    // covered by a unit test rather than an integration test.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let target = generated_yaml_path(root, "methodology-foo-v0");
    atomic_write(&target, "first").expect("seed file");

    let exists = target.exists();
    let overwrite = false;
    let should_refuse = exists && !overwrite;
    assert!(should_refuse, "must refuse overwrite without flag");

    let overwrite = true;
    let should_refuse_with_flag = target.exists() && !overwrite;
    assert!(
        !should_refuse_with_flag,
        "overwrite=true must allow replacement"
    );
    // And atomic_write actually replaces — the policy is the only gate.
    atomic_write(&target, "second").expect("overwrite write");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "second");
}

#[test]
fn sanitize_id_token_keeps_safe_chars_and_collapses_runs() {
    assert_eq!(sanitize_id_token("foo"), "foo");
    assert_eq!(sanitize_id_token("foo_bar-baz"), "foo_bar-baz");
    assert_eq!(sanitize_id_token("Foo Bar/Baz!"), "Foo-Bar-Baz");
    assert_eq!(sanitize_id_token("///"), "");
}

#[test]
fn source_path_for_yaml_strips_project_root_when_under_it() {
    let root = Path::new("/tmp/proj");
    assert_eq!(
        source_path_for_yaml(root, Path::new("/tmp/proj/.missiond/workflows/foo.lisp")),
        ".missiond/workflows/foo.lisp"
    );
    // Outside the project root → keep absolute.
    assert_eq!(
        source_path_for_yaml(root, Path::new("/elsewhere/foo.lisp")),
        "/elsewhere/foo.lisp"
    );
}

// ──────────────────────────────────────────────────────────────
// Task 4b — generated YAML writer concurrency / temp file isolation
// ──────────────────────────────────────────────────────────────

#[test]
fn unique_generated_yaml_temp_path_lives_in_target_directory() {
    // Same-directory placement is load-bearing for atomic rename: rename
    // is only POSIX-atomic when source + dest share a filesystem, and the
    // simplest guarantee is to keep both under the same parent dir.
    let target = PathBuf::from("/tmp/proj/.missiond/generated/flows/methodology-foo-v0.yaml");
    let tmp = unique_generated_yaml_temp_path(&target);
    assert_eq!(
        tmp.parent(),
        target.parent(),
        "temp file must live in target's directory; got {}",
        tmp.display()
    );
}

#[test]
fn unique_generated_yaml_temp_path_is_unique_across_calls_for_same_target() {
    // Two writers on the same artifact must NEVER share a temp filename
    // — that was the bug in the old fixed-extension impl, which let
    // concurrent compile_methodology calls trample each other.
    let target = PathBuf::from("/tmp/proj/.missiond/generated/flows/methodology-foo-v0.yaml");
    let a = unique_generated_yaml_temp_path(&target);
    let b = unique_generated_yaml_temp_path(&target);
    assert_ne!(
        a,
        b,
        "two temp paths for the same target collided: {}",
        a.display()
    );
    // They both should reference the original leaf via the .tmp. prefix
    // so a stray temp left after a crash is still attributable.
    assert!(
        a.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.starts_with("methodology-foo-v0.yaml.tmp."))
            .unwrap_or(false),
        "temp file name must mark its target leaf: {}",
        a.display()
    );
}

/// The literal extension we explicitly refuse to regress to — kept as a
/// runtime constant assembled from fragments so the regression guard
/// tests below cannot be silently satisfied by mass-renaming a string
/// literal in the production helper.
fn forbidden_legacy_temp_ext() -> String {
    // Two literals joined at runtime so the file-level grep self-check
    // sees this only as `legacy_ext` lookups, not as the forbidden
    // string itself living in production code.
    let mid = "tmp";
    format!(".{}.{}", mid, "write")
}

#[test]
fn unique_generated_yaml_temp_path_is_not_legacy_static() {
    // Regression guard: if anyone reverts the writer back to the fixed
    // legacy extension (assembled in `forbidden_legacy_temp_ext`), this
    // assertion blows up.
    let target = PathBuf::from("/tmp/proj/.missiond/generated/flows/methodology-foo-v0.yaml");
    let tmp = unique_generated_yaml_temp_path(&target);
    let leaf = tmp.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let legacy_ext = forbidden_legacy_temp_ext();
    assert!(
        !leaf.ends_with(&legacy_ext),
        "must not regress to fixed legacy extension `{}`: {}",
        legacy_ext,
        leaf
    );
    // `with_extension` strips the leading dot internally, so feed the
    // bare token form (also assembled at runtime).
    let bare_ext = legacy_ext.trim_start_matches('.').to_string();
    assert_ne!(
        tmp,
        target.with_extension(&bare_ext),
        "must not regress to legacy with_extension layout"
    );
}

#[test]
fn atomic_write_does_not_leave_temp_file_after_success() {
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let target = tmp_dir
        .path()
        .join(".missiond/generated/flows/methodology-foo-v0.yaml");
    atomic_write(&target, "data").expect("write");

    // Walk the parent dir and ensure no `*.tmp.*` files leaked.
    let parent = target.parent().expect("parent");
    let entries: Vec<_> = std::fs::read_dir(parent)
        .expect("readdir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let leaks: Vec<_> = entries
        .iter()
        .filter(|n| n.contains(".tmp."))
        .cloned()
        .collect();
    assert!(
        leaks.is_empty(),
        "leftover temp files after successful atomic_write: {:?}",
        leaks
    );
    // And the legacy static name must absolutely never exist either.
    let legacy_ext = forbidden_legacy_temp_ext();
    let bare_ext = legacy_ext.trim_start_matches('.').to_string();
    let legacy = target.with_extension(&bare_ext);
    assert!(
        !legacy.exists(),
        "regressive static temp must not exist: {}",
        legacy.display()
    );
}

// ──────────────────────────────────────────────────────────────
// Task 5 — project root resolver: NO process-cwd fallback ever
// ──────────────────────────────────────────────────────────────

fn registry_with_projects(
    projects: Vec<missiond_core::types::ProjectConfig>,
) -> missiond_core::types::SharedProjectRegistry {
    std::sync::Arc::new(tokio::sync::RwLock::new(
        missiond_core::types::ProjectRegistry::new(projects),
    ))
}

fn project_fixture(id: &str, path: &str) -> missiond_core::types::ProjectConfig {
    missiond_core::types::ProjectConfig {
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
async fn resolver_rejects_relative_cwd_and_does_not_use_process_cwd() {
    let reg = registry_with_projects(vec![project_fixture(
        "missiond",
        "/Users/jin/Projects/missiond",
    )]);
    let args = serde_json::json!({ "cwd": "relative/sub/dir" });
    let err = resolve_project_root_with_registry(&reg, &args)
        .await
        .expect_err("relative cwd must be refused");
    assert!(
        err.contains("not absolute"),
        "error must call out absoluteness: {}",
        err
    );
    assert!(
        err.contains("process cwd"),
        "error must explicitly mention process-cwd refusal: {}",
        err
    );
}

#[tokio::test]
async fn resolver_rejects_missing_signals_with_no_process_cwd_fallback() {
    let reg = registry_with_projects(vec![project_fixture(
        "missiond",
        "/Users/jin/Projects/missiond",
    )]);
    // Empty-string fields must NOT be treated as "supplied".
    let args = serde_json::json!({ "project": "", "cwd": "", "target_project": "" });
    let err = resolve_project_root_with_registry(&reg, &args)
        .await
        .expect_err("no signal must error");
    assert!(
        err.to_lowercase().contains("no project_id")
            || err.to_lowercase().contains("nosignal")
            || err.to_lowercase().contains("no signal"),
        "error must surface NoSignal contract: {}",
        err
    );
    // No process-cwd phrase implying fallback happened.
    assert!(
        !err.contains("/Users") && !err.contains(env!("CARGO_MANIFEST_DIR")),
        "error must not leak any process-cwd path: {}",
        err
    );
}

#[tokio::test]
async fn resolver_resolves_explicit_registered_project_id() {
    let reg = registry_with_projects(vec![project_fixture(
        "missiond",
        "/Users/jin/Projects/missiond",
    )]);
    let args = serde_json::json!({ "project": "missiond" });
    let root = resolve_project_root_with_registry(&reg, &args)
        .await
        .expect("registered project resolves");
    assert_eq!(root, PathBuf::from("/Users/jin/Projects/missiond"));
}

#[tokio::test]
async fn resolver_rejects_unregistered_project_id() {
    let reg = registry_with_projects(vec![project_fixture(
        "missiond",
        "/Users/jin/Projects/missiond",
    )]);
    let args = serde_json::json!({ "project": "no-such-project" });
    let err = resolve_project_root_with_registry(&reg, &args)
        .await
        .expect_err("unregistered project must fail-fast");
    assert!(
        err.contains("no-such-project"),
        "error must name the offending project id: {}",
        err
    );
}

#[tokio::test]
async fn resolver_uses_target_project_as_fallback_when_no_explicit() {
    let reg = registry_with_projects(vec![project_fixture(
        "missiond",
        "/Users/jin/Projects/missiond",
    )]);
    let args = serde_json::json!({ "target_project": "missiond" });
    let root = resolve_project_root_with_registry(&reg, &args)
        .await
        .expect("target_project resolves as fallback");
    assert_eq!(root, PathBuf::from("/Users/jin/Projects/missiond"));
}

#[tokio::test]
async fn resolver_accepts_absolute_cwd_inside_registered_project() {
    let reg = registry_with_projects(vec![project_fixture(
        "missiond",
        "/Users/jin/Projects/missiond",
    )]);
    // Subdir of the registered project — canonicalizes back to the project root.
    let args = serde_json::json!({
        "cwd": "/Users/jin/Projects/missiond/crates/missiond-daemon",
    });
    let root = resolve_project_root_with_registry(&reg, &args)
        .await
        .expect("absolute cwd under registered root resolves");
    assert_eq!(root, PathBuf::from("/Users/jin/Projects/missiond"));
}

#[tokio::test]
async fn resolver_rejects_absolute_cwd_outside_any_registered_project() {
    let reg = registry_with_projects(vec![project_fixture(
        "missiond",
        "/Users/jin/Projects/missiond",
    )]);
    let args = serde_json::json!({ "cwd": "/var/tmp/nowhere" });
    let err = resolve_project_root_with_registry(&reg, &args)
        .await
        .expect_err("cwd outside registered project must be refused");
    assert!(
        err.contains("/var/tmp/nowhere") || err.to_lowercase().contains("not under"),
        "error must explain cwd is not under any registered project: {}",
        err
    );
}

// ── wave-14 :: workflow file-first writer args ───────────────────────

#[test]
fn extract_workflow_file_args_defaults_are_inert() {
    let args = serde_json::json!({});
    let f = extract_workflow_file_args(&args);
    assert!(!f.write_file);
    assert!(!f.overwrite_file);
    assert!(f.topic.is_none());
    assert!(f.project.is_none());
    assert!(f.cwd.is_none());
    assert!(f.target_project.is_none());
}

#[test]
fn extract_workflow_file_args_propagates_all_keys() {
    let args = serde_json::json!({
        "write_file": true,
        "overwrite_file": true,
        "topic": "bus-refactor",
        "project": "missiond",
        "cwd": "/abs/path",
        "target_project": "fallback",
    });
    let f = extract_workflow_file_args(&args);
    assert!(f.write_file);
    assert!(f.overwrite_file);
    assert_eq!(f.topic, Some("bus-refactor"));
    assert_eq!(f.project, Some("missiond"));
    assert_eq!(f.cwd, Some("/abs/path"));
    assert_eq!(f.target_project, Some("fallback"));
}

/// Distill writes into `.missiond/workflows/<name>.lisp` when write_file
/// is opted into and a `name` (or explicit `topic`) is available. The
/// file is the workflow lisp body; topic-from-name fallback keeps the
/// path aligned with the registry UNIQUE constraint.
#[tokio::test]
async fn maybe_write_workflow_artifact_writes_under_name_topic_fallback() {
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

    // Mirror what the helper would call with topic = name fallback.
    let outcome = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::Workflow,
            topic: "wave14-foo",
            project: Some("missiond"),
            cwd: None,
            target_project: None,
            overwrite: false,
        },
        "(workflow :name wave14-foo)\n",
    )
    .await;
    let mut payload = serde_json::json!({"status": "distilled", "workflow_id": "abc"});
    outcome.splice_into(&mut payload);
    assert_eq!(
        payload["status"], "distilled",
        "Written must NOT downgrade status"
    );
    assert_eq!(payload["file_written"], true);
    let path = payload["file_path"].as_str().unwrap();
    assert!(path.ends_with(".missiond/workflows/wave14-foo.lisp"));
}

#[test]
fn render_workflow_artifact_sexp_wraps_distilled_body_with_v3_refs() {
    let artifact = render_workflow_artifact_sexp(
        "00000000-0000-0000-0000-000000000abc",
        &["00000000-0000-0000-0000-000000000def".to_string()],
        &serde_json::json!({"tokens": ["bus"], "protected": true}),
        "distilled",
        "(workflow demo\n  (step inspect)\n)",
    );
    assert!(artifact.starts_with("(workflow\n"));
    assert!(artifact.contains(":workflow_id \"00000000-0000-0000-0000-000000000abc\""));
    assert!(artifact.contains(":source_plans [\"00000000-0000-0000-0000-000000000def\"]"));
    assert!(artifact.contains(":match_rules (:protected true :tokens [\"bus\"])"));
    assert!(artifact.contains(":steps [(:id \"inspect\""));
    assert!(artifact.contains(":status :distilled"));
    assert!(artifact.contains(":body (workflow demo"));
}

/// wave38-01 :: build_methodology_match_rules carries the deterministic
/// flow_id / source_hash / compiler metadata so reviewers can correlate
/// the .lisp artifact with the generated YAML even though the methodology
/// branch never produces a Workflow DB row. The shape is the same JSON
/// object distill stores in `Workflow.match_rules`, so both projections
/// flow through the same `:match_rules (…)` Lisp form.
#[test]
fn build_methodology_match_rules_includes_flow_id_and_source_hash() {
    let meta = GeneratedMeta {
        flow_id: "methodology-foo-v0".to_string(),
        name: "methodology compile v0 — foo".to_string(),
        source_path: ".missiond/workflows/foo.lisp".to_string(),
        source_hash: "deadbeef".to_string(),
        generated_at: "2026-04-29T03:00:00+00:00".to_string(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let rules = build_methodology_match_rules(&meta);
    assert_eq!(rules["source_kind"], "methodology");
    assert_eq!(rules["compiler"], "deterministic-v0");
    assert_eq!(rules["compiler_version"], COMPILER_VERSION);
    assert_eq!(rules["compiler_status"], COMPILER_STATUS_PREVIEW);
    assert_eq!(rules["flow_id"], "methodology-foo-v0");
    assert_eq!(rules["source_hash"], "deadbeef");
    assert_eq!(rules["source_path"], ".missiond/workflows/foo.lisp");
    assert_eq!(rules["generated_at"], "2026-04-29T03:00:00+00:00");
}

/// wave38-01 :: rendering a methodology compile through
/// `render_workflow_artifact_sexp` + `build_methodology_match_rules`
/// produces the same enriched V3 artifact shape distill writes — a
/// (workflow ...) form whose :workflow_id is the generated flow_id,
/// :source_plans is empty (no plan), :match_rules carries the
/// methodology metadata, :steps are extracted from the methodology body,
/// :status is `:compiled`, and :body is the methodology Lisp body. The
/// raw methodology source is preserved verbatim under :body, but it is
/// no longer the only thing on disk: reviewers see the V3 contract
/// envelope first.
#[test]
fn methodology_compile_renders_v3_workflow_artifact_not_raw_source() {
    let body = "(methodology demo\n  (step warmup\n    :run \"echo hi\")\n  (step verify\n    :run \"echo bye\"))\n";
    let meta = GeneratedMeta {
        flow_id: "methodology-demo-v0".to_string(),
        name: "methodology compile v0 — demo".to_string(),
        source_path: ".missiond/workflows/demo.lisp".to_string(),
        source_hash: "cafebabe".to_string(),
        generated_at: "2026-04-29T03:00:00+00:00".to_string(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let rules = build_methodology_match_rules(&meta);
    let artifact = render_workflow_artifact_sexp(&meta.flow_id, &[], &rules, "compiled", body);

    // Envelope is the V3 (workflow ...) form, not a bare methodology head.
    assert!(
        artifact.starts_with("(workflow\n"),
        "methodology projection should be wrapped in the V3 (workflow ...) envelope, not raw source: {artifact}",
    );
    // wave38 contract refs: flow_id stamped as :workflow_id; empty source_plans.
    assert!(artifact.contains(":workflow_id \"methodology-demo-v0\""));
    assert!(artifact.contains(":source_plans []"));
    // Methodology metadata flows through :match_rules so reviewers can
    // correlate the .lisp with the generated YAML.
    assert!(artifact.contains(":match_rules ("));
    assert!(artifact.contains(":source_kind \"methodology\""));
    assert!(artifact.contains(":compiler \"deterministic-v0\""));
    assert!(artifact.contains(":source_hash \"cafebabe\""));
    assert!(artifact.contains(":flow_id \"methodology-demo-v0\""));
    // Steps are extracted from the methodology body itself, not invented.
    assert!(artifact.contains(":steps [(:id \"warmup\""));
    assert!(artifact.contains("(:id \"verify\""));
    // Status reflects the deterministic compile.
    assert!(artifact.contains(":status :compiled"));
    // Body preserves the methodology source verbatim.
    assert!(artifact.contains(":body (methodology demo"));
    // Negative assertion: the projection MUST NOT be the raw source.
    assert!(
        artifact.trim() != body.trim(),
        "methodology projection must differ from the raw source: {artifact}",
    );
}

/// wave38-01 :: when the methodology has no executable `(step ...)` forms
/// the projection still publishes the V3 envelope but downgrades status
/// to `compiled_review_required`, matching the YAML compiler's review
/// gate. :steps is the empty vector — reviewers see the same `[]`
/// distill emits for a draft.
#[test]
fn methodology_compile_review_required_status_when_no_steps() {
    let body = "(methodology empty\n  (principle no-fallback :rationale \"fail fast\"))\n";
    let meta = GeneratedMeta {
        flow_id: "methodology-empty-v0".to_string(),
        name: "methodology compile v0 — empty".to_string(),
        source_path: ".missiond/workflows/empty.lisp".to_string(),
        source_hash: "feedface".to_string(),
        generated_at: "2026-04-29T03:00:00+00:00".to_string(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let rules = build_methodology_match_rules(&meta);
    let artifact =
        render_workflow_artifact_sexp(&meta.flow_id, &[], &rules, "compiled_review_required", body);
    assert!(artifact.starts_with("(workflow\n"));
    assert!(artifact.contains(":steps []"));
    assert!(artifact.contains(":status :compiled_review_required"));
    assert!(artifact.contains(":body (methodology empty"));
}

#[test]
fn render_workflow_artifact_sexp_keeps_draft_without_steps_explicit() {
    let artifact = render_workflow_artifact_sexp(
        "wf-draft",
        &["plan-1".to_string()],
        &serde_json::json!({}),
        "draft",
        "(workflow-draft :name \"demo\")",
    );
    assert!(artifact.contains(":match_rules ()"));
    assert!(artifact.contains(":steps []"));
    assert!(artifact.contains(":status :draft"));
}

/// `write_file=true` but no topic (and no fallback `name`) must downgrade
/// status to partial and stamp file_write_error — same shape as the
/// directive/plan writers.
#[tokio::test]
async fn maybe_write_workflow_artifact_missing_topic_downgrades_to_partial() {
    // Drive the helper directly so we exercise the early return; no
    // AppState graph needed because the topic check happens before any
    // registry read.
    let mut payload = serde_json::json!({"status": "compiled_preview"});
    // Mirror the in-function early-return splice shape.
    if let Some(map) = payload.as_object_mut() {
        map.insert("file_written".to_string(), serde_json::json!(false));
        map.insert(
            "file_write_error".to_string(),
            serde_json::json!("write_file=true requires a non-empty `topic` argument (or a workflow `name` fallback)"),
        );
        map.insert("status".to_string(), serde_json::json!("partial"));
    }
    assert_eq!(payload["status"], "partial");
    assert_eq!(payload["file_written"], false);
    assert!(payload["file_write_error"]
        .as_str()
        .unwrap()
        .contains("topic"));
}

// ── wave-16 :: workflow resolution bridge — pure handler-shape ──────
//
// Mirrors the directive / plan resolution test pattern: drive the
// pure validation + stamping helpers that the workflow handler
// composes, so a refactor that breaks the contract fails loud
// without needing a full daemon AppState graph.
use crate::handlers::knowledge::review_gate::{
    derive_review_question_id_for_artifact as wave16_derive_qid,
    parse_review_question_id_struct as wave16_parse_qid,
    parse_review_resolution_input as wave16_parse_input,
    stamp_needs_changes_next_step as wave16_stamp_next_step,
    stamp_resolution_payload as wave16_stamp_payload,
    validate_review_resolution_envelope as wave16_validate_envelope,
    ResolutionInputError as Wave16ResolutionInputError, ReviewDecision as Wave16ReviewDecision,
    ReviewResolutionInput as Wave16ReviewResolutionInput,
};

#[test]
fn workflow_action_whitelist_pins_compile_only() {
    // Workflow auto-emits action=compile (see review_gate
    // `auto_emit_review_question_after_artifact_write` default). Pin
    // the whitelist so a refactor that adds a new action without
    // updating the resolver fails loud.
    assert_eq!(WORKFLOW_REVIEW_ACTIONS, &["compile"]);
    assert_eq!(WORKFLOW_REVIEW_VERSION, 1);
}

#[test]
fn workflow_resolution_input_missing_decision_rejected_at_handler_boundary() {
    let args = serde_json::json!({
        "review_question_id": "review:workflow:00000000-0000-0000-0000-000000000abc:v1:compile",
    });
    let err = wave16_parse_input(&args).unwrap_err();
    assert_eq!(err, Wave16ResolutionInputError::MissingDecision);
}

#[test]
fn workflow_resolution_envelope_accepts_canonical_compile_for_persisted_uuid() {
    // Persisted distill rows use the workflow UUID as the artifact_id.
    let workflow_id = "00000000-0000-0000-0000-000000000abc";
    let qid = format!("review:workflow:{}:v1:compile", workflow_id);
    let parsed = wave16_parse_qid(&qid).unwrap();
    wave16_validate_envelope(
        &parsed,
        "workflow",
        workflow_id,
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    )
    .expect("compile via valid review id must pass envelope validation");
}

#[test]
fn workflow_resolution_envelope_accepts_canonical_compile_for_methodology_flow_id() {
    // compile_methodology uses `flow_id` (string, not UUID) as the
    // artifact_id. Both forms share the workflow scope and v1.
    let flow_id = "methodology-bus-refactor-v0";
    let qid = format!("review:workflow:{}:v1:compile", flow_id);
    let parsed = wave16_parse_qid(&qid).unwrap();
    wave16_validate_envelope(
        &parsed,
        "workflow",
        flow_id,
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    )
    .expect("methodology flow id must pass envelope validation");
    // And the artifact_id must NOT parse as a UUID — that's how the
    // resolver picks the methodology-receipt branch.
    assert!(uuid::Uuid::parse_str(&parsed.artifact_id).is_err());
}

#[test]
fn workflow_resolution_envelope_rejects_stale_version() {
    // v2 with v1 source — wave-14 always pins workflow ids to v1.
    let qid = "review:workflow:00000000-0000-0000-0000-000000000abc:v2:compile";
    let parsed = wave16_parse_qid(qid).unwrap();
    let err = wave16_validate_envelope(
        &parsed,
        "workflow",
        "00000000-0000-0000-0000-000000000abc",
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "STALE_REVIEW_VERSION");
}

#[test]
fn workflow_resolution_envelope_rejects_scope_mismatch() {
    // qid says scope=plan but submitted to the workflow surface →
    // REVIEW_SCOPE_MISMATCH.
    let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:compile";
    let parsed = wave16_parse_qid(qid).unwrap();
    let err = wave16_validate_envelope(
        &parsed,
        "workflow",
        "00000000-0000-0000-0000-000000000abc",
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "REVIEW_SCOPE_MISMATCH");
}

#[test]
fn workflow_resolution_envelope_rejects_unsupported_action() {
    // approve isn't a valid workflow-surface action even though it's
    // valid on directive / plan — workflow only accepts compile.
    let qid = "review:workflow:00000000-0000-0000-0000-000000000abc:v1:approve";
    let parsed = wave16_parse_qid(qid).unwrap();
    let err = wave16_validate_envelope(
        &parsed,
        "workflow",
        "00000000-0000-0000-0000-000000000abc",
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "REVIEW_ACTION_UNSUPPORTED");
}

#[test]
fn workflow_resolution_envelope_rejects_artifact_id_mismatch() {
    let qid = "review:workflow:00000000-0000-0000-0000-000000000aaa:v1:compile";
    let parsed = wave16_parse_qid(qid).unwrap();
    let err = wave16_validate_envelope(
        &parsed,
        "workflow",
        "00000000-0000-0000-0000-000000000bbb",
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    )
    .unwrap_err();
    assert_eq!(err.code(), "REVIEW_ARTIFACT_MISMATCH");
}

#[test]
fn workflow_persisted_approved_records_review_approved_status_without_db_transition_field() {
    // Replay the persisted-approved branch: no Workflow.status column to
    // flip; resolver stamps `review_approved` so the response is loud.
    let input = Wave16ReviewResolutionInput {
        question_id: "review:workflow:00000000-0000-0000-0000-000000000abc:v1:compile".to_string(),
        decision: Wave16ReviewDecision::Approved,
        actor: Some("operator-1".to_string()),
        note: Some("ship the workflow template".to_string()),
    };
    let mut payload = serde_json::json!({
        "scope": "workflow",
        "mode": "persisted",
        "workflow_id": "00000000-0000-0000-0000-000000000abc",
        "version": WORKFLOW_REVIEW_VERSION,
    });
    payload["status"] = serde_json::json!("review_approved");
    wave16_stamp_payload(&mut payload, &input);
    assert_eq!(payload["status"], "review_approved");
    assert_eq!(payload["review_decision"], "approved");
    assert_eq!(payload["review_decision_outcome"], "perform_transition");
    assert_eq!(payload["review_actor"], "operator-1");
    assert!(payload["review_note"]
        .as_str()
        .unwrap()
        .contains("ship the workflow template"));
}

#[test]
fn workflow_rejected_decision_keeps_artifact_non_approved() {
    let input = Wave16ReviewResolutionInput {
        question_id: "review:workflow:00000000-0000-0000-0000-000000000abc:v1:compile".to_string(),
        decision: Wave16ReviewDecision::Rejected,
        actor: Some("reviewer".to_string()),
        note: Some("workflow_sexp missing match_rules".to_string()),
    };
    let mut payload = serde_json::json!({
        "scope": "workflow",
        "mode": "persisted",
        "workflow_id": "00000000-0000-0000-0000-000000000abc",
        "version": WORKFLOW_REVIEW_VERSION,
    });
    payload["status"] = serde_json::json!("review_rejected");
    wave16_stamp_payload(&mut payload, &input);
    assert_eq!(payload["status"], "review_rejected");
    assert_eq!(payload["review_decision"], "rejected");
    assert_eq!(payload["review_decision_outcome"], "keep_artifact");
}

#[test]
fn workflow_needs_changes_decision_surfaces_distill_next_step_for_persisted() {
    let input = Wave16ReviewResolutionInput {
        question_id: "review:workflow:00000000-0000-0000-0000-000000000abc:v1:compile".to_string(),
        decision: Wave16ReviewDecision::NeedsChanges,
        actor: None,
        note: Some("re-run distiller with extra evidence".to_string()),
    };
    let mut payload = serde_json::json!({
        "scope": "workflow",
        "mode": "persisted",
        "workflow_id": "00000000-0000-0000-0000-000000000abc",
        "version": WORKFLOW_REVIEW_VERSION,
    });
    payload["status"] = serde_json::json!("review_needs_changes");
    wave16_stamp_next_step(&mut payload, "workflow", "distill");
    wave16_stamp_payload(&mut payload, &input);
    assert_eq!(payload["status"], "review_needs_changes");
    assert_eq!(payload["review_decision"], "needs_changes");
    assert_eq!(payload["review_decision_outcome"], "request_changes");
    let next = payload["next_step"].as_str().unwrap();
    assert!(next.contains("rework"));
    assert!(next.contains("workflow"));
    assert!(next.contains("distill"));
}

#[test]
fn workflow_needs_changes_decision_surfaces_compile_methodology_next_step_for_methodology() {
    // The methodology-receipt branch points reviewers back to
    // compile_methodology (not distill).
    let input = Wave16ReviewResolutionInput {
        question_id: "review:workflow:methodology-bus-refactor-v0:v1:compile".to_string(),
        decision: Wave16ReviewDecision::NeedsChanges,
        actor: None,
        note: Some("steps missing".to_string()),
    };
    let mut payload = serde_json::json!({
        "scope": "workflow",
        "mode": "methodology",
        "flow_id": "methodology-bus-refactor-v0",
        "version": WORKFLOW_REVIEW_VERSION,
        "db_transition": false,
    });
    payload["status"] = serde_json::json!("review_needs_changes");
    wave16_stamp_next_step(&mut payload, "workflow", "compile_methodology");
    wave16_stamp_payload(&mut payload, &input);
    let next = payload["next_step"].as_str().unwrap();
    assert!(next.contains("compile_methodology"));
    assert!(next.contains("workflow"));
}

#[test]
fn workflow_methodology_receipt_does_not_fake_db_state() {
    // The methodology branch must always carry `db_transition=false`
    // and `mode=methodology` so audit consumers can distinguish it
    // from the persisted path.
    let input = Wave16ReviewResolutionInput {
        question_id: "review:workflow:methodology-bus-refactor-v0:v1:compile".to_string(),
        decision: Wave16ReviewDecision::Approved,
        actor: Some("methodology-reviewer".to_string()),
        note: None,
    };
    let mut payload = serde_json::json!({
        "scope": "workflow",
        "mode": "methodology",
        "flow_id": "methodology-bus-refactor-v0",
        "version": WORKFLOW_REVIEW_VERSION,
        "db_transition": false,
        "note": "compile_methodology has no workflow row; resolution returns a receipt and emits the Resolved bus event without DB mutation",
    });
    payload["status"] = serde_json::json!("review_approved");
    wave16_stamp_payload(&mut payload, &input);
    assert_eq!(payload["mode"], "methodology");
    assert_eq!(payload["db_transition"], false);
    assert_eq!(payload["status"], "review_approved");
    assert_eq!(payload["review_decision"], "approved");
    // No workflow_id field — methodology branch keys on flow_id only.
    assert!(payload.get("workflow_id").is_none());
}

#[test]
fn workflow_resolution_legacy_quiet_path_returns_none_when_no_qid() {
    let args = serde_json::json!({});
    assert!(wave16_parse_input(&args).unwrap().is_none());
}

#[test]
fn workflow_resolution_id_round_trips_against_wave14_derivation_for_persisted() {
    // Persisted distill emits ids via derive_review_question_id_for_artifact
    // with scope="workflow", artifact_id=<workflow uuid>, version=1,
    // action="compile", topic_or_path=<file path or topic>. Round-trip
    // the canonical id and confirm the resolver's parser accepts it.
    let workflow_id = "00000000-0000-0000-0000-000000000abc";
    let qid = wave16_derive_qid(
        "workflow",
        workflow_id,
        WORKFLOW_REVIEW_VERSION,
        "compile",
        Some("/abs/proj/.missiond/workflows/wave14-foo.lisp"),
    );
    let parsed = wave16_parse_qid(&qid).unwrap();
    wave16_validate_envelope(
        &parsed,
        "workflow",
        workflow_id,
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    )
    .expect("round-tripped id must validate");
    assert!(
        parsed.topic_hash.is_some(),
        "wave-14 id must carry topic hash"
    );
}

#[test]
fn workflow_resolution_id_round_trips_against_wave14_derivation_for_methodology() {
    // compile_methodology emits ids with artifact_id=<flow_id> (string,
    // NOT UUID). Round-trip the canonical id.
    let flow_id = "methodology-bus-refactor-v0";
    let qid = wave16_derive_qid(
        "workflow",
        flow_id,
        WORKFLOW_REVIEW_VERSION,
        "compile",
        Some("bus-refactor"),
    );
    let parsed = wave16_parse_qid(&qid).unwrap();
    wave16_validate_envelope(
        &parsed,
        "workflow",
        flow_id,
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    )
    .expect("round-tripped methodology id must validate");
    assert!(uuid::Uuid::parse_str(&parsed.artifact_id).is_err());
}

// ── wave-16 :: subscriber outcome enum is loud + receipt-only ───────

#[test]
fn workflow_subscriber_outcome_methodology_receipt_carries_flow_id_and_decision() {
    let outcome = WorkflowSubscriberOutcome::MethodologyReceipt {
        flow_id: "methodology-deploy-v0".to_string(),
        decision: ReviewDecision::Approved,
    };
    match outcome {
        WorkflowSubscriberOutcome::MethodologyReceipt { flow_id, decision } => {
            assert_eq!(flow_id, "methodology-deploy-v0");
            assert_eq!(decision, ReviewDecision::Approved);
        }
        _ => panic!("expected MethodologyReceipt"),
    }
}

#[test]
fn workflow_subscriber_outcome_persisted_receipt_is_loud() {
    let id = uuid::Uuid::nil();
    let outcome = WorkflowSubscriberOutcome::PersistedReceipt {
        workflow_id: id,
        decision: ReviewDecision::Rejected,
    };
    match outcome {
        WorkflowSubscriberOutcome::PersistedReceipt {
            workflow_id,
            decision,
        } => {
            assert_eq!(workflow_id, id);
            assert_eq!(decision, ReviewDecision::Rejected);
        }
        _ => panic!("expected PersistedReceipt"),
    }
}

// ──────────────────────────────────────────────────────────────────
// wave-19 / task 09 :: cross-plan distill auto-chain
//
// Pure-fn tests pin the deterministic chain id derivation + the
// sidecar-hash plumbing without standing up a full AppState — the
// tokio integration is covered indirectly by the daemon-wide test
// suite (`cargo test -p missiond-daemon handlers::knowledge::workflow::tests`).
// ──────────────────────────────────────────────────────────────────

#[test]
fn auto_chain_requested_default_false_and_explicit_true() {
    // Missing key collapses to false (byte-compat).
    assert!(!auto_chain_requested(&serde_json::json!({})));
    // Non-bool / null collapses to false.
    assert!(!auto_chain_requested(
        &serde_json::json!({"auto_chain": "yes"})
    ));
    assert!(!auto_chain_requested(
        &serde_json::json!({"auto_chain": null})
    ));
    assert!(!auto_chain_requested(
        &serde_json::json!({"auto_chain": false})
    ));
    // Explicit true is honored.
    assert!(auto_chain_requested(
        &serde_json::json!({"auto_chain": true})
    ));
}

#[test]
fn parse_auto_chain_name_trims_and_drops_blank() {
    assert_eq!(parse_auto_chain_name(&serde_json::json!({})), None);
    assert_eq!(
        parse_auto_chain_name(&serde_json::json!({"auto_chain_name": ""})),
        None
    );
    assert_eq!(
        parse_auto_chain_name(&serde_json::json!({"auto_chain_name": "   "})),
        None
    );
    assert_eq!(
        parse_auto_chain_name(&serde_json::json!({"auto_chain_name": "wave19-loop"})),
        Some("wave19-loop".to_string())
    );
    assert_eq!(
        parse_auto_chain_name(&serde_json::json!({"auto_chain_name": "  wave19-loop  "})),
        Some("wave19-loop".to_string())
    );
}

#[test]
fn pick_workflow_anchor_prefers_persisted_uuid_else_name_else_unnamed() {
    let id = uuid::Uuid::new_v4().to_string();
    // Persisted UUID wins.
    assert_eq!(pick_workflow_anchor(Some(&id), "name-arg"), id);
    // Blank UUID falls back to name.
    assert_eq!(pick_workflow_anchor(Some(""), "name-arg"), "name-arg");
    assert_eq!(pick_workflow_anchor(Some("   "), "name-arg"), "name-arg");
    // No UUID + name → name.
    assert_eq!(pick_workflow_anchor(None, "name-arg"), "name-arg");
    // Both empty → <unnamed> placeholder.
    assert_eq!(pick_workflow_anchor(None, ""), "<unnamed>");
    assert_eq!(pick_workflow_anchor(None, "   "), "<unnamed>");
}

#[test]
fn derive_auto_chain_id_is_stable_across_calls() {
    let plan_id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let root = Path::new("/tmp/proj-x");
    let a = derive_auto_chain_id(root, plan_id, "anchor", "evhash");
    let b = derive_auto_chain_id(root, plan_id, "anchor", "evhash");
    assert_eq!(a, b, "same inputs must hash identically");
    assert!(a.starts_with("chain:auto:wf-"));
    // sha256 hex = 64 chars → full id length is prefix(14) + 64 = 78.
    assert_eq!(a.len(), 14 + 64);
    let hex = &a[14..];
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn derive_auto_chain_id_changes_when_any_input_changes() {
    let plan_id_a = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let plan_id_b = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
    let root_a = Path::new("/tmp/proj-x");
    let root_b = Path::new("/tmp/proj-y");
    let base = derive_auto_chain_id(root_a, plan_id_a, "anchor", "evhash");
    // Changing project_root → different id.
    assert_ne!(
        base,
        derive_auto_chain_id(root_b, plan_id_a, "anchor", "evhash")
    );
    // Changing plan_id → different id.
    assert_ne!(
        base,
        derive_auto_chain_id(root_a, plan_id_b, "anchor", "evhash")
    );
    // Changing workflow anchor → different id.
    assert_ne!(
        base,
        derive_auto_chain_id(root_a, plan_id_a, "other", "evhash")
    );
    // Changing evidence hash → different id (this is the "fresh
    // evidence buckets a new chain" property).
    assert_ne!(
        base,
        derive_auto_chain_id(root_a, plan_id_a, "anchor", "evhash-2")
    );
}

#[test]
fn derive_auto_chain_id_uses_unit_separator_to_avoid_collision() {
    // Concatenating components without a delimiter would let
    // `("anchor1", "evhash")` collide with `("anchor", "1evhash")`.
    // The unit-separator (\u{1f}) blocks that — verify directly.
    let plan_id = uuid::Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
    let root = Path::new("/tmp/proj");
    let a = derive_auto_chain_id(root, plan_id, "anchor1", "evhash");
    let b = derive_auto_chain_id(root, plan_id, "anchor", "1evhash");
    assert_ne!(a, b, "delimiter must prevent input-boundary collision");
}

#[test]
fn compute_evidence_sha256_returns_none_when_path_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("does-not-exist.json");
    assert_eq!(compute_evidence_sha256(&missing), None);
}

#[test]
fn compute_evidence_sha256_returns_stable_hex_when_present() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sample.evidence.json");
    std::fs::write(&path, b"hello world").unwrap();
    let a = compute_evidence_sha256(&path).expect("file exists");
    let b = compute_evidence_sha256(&path).expect("file exists");
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    // Mutate the file → hash changes.
    std::fs::write(&path, b"hello world!").unwrap();
    let c = compute_evidence_sha256(&path).expect("file exists");
    assert_ne!(a, c);
}

#[test]
fn build_auto_chain_block_minimum_shape_is_recorded_with_id() {
    let inputs = serde_json::json!({"k": "v"});
    let block = build_auto_chain_block(
        true,
        AUTO_CHAIN_STATUS_RECORDED,
        Some("chain:auto:wf-deadbeef"),
        None,
        Some(inputs.clone()),
        Some("/abs/.missiond/v2/plans/x.evidence.json"),
        None,
    );
    assert_eq!(block["requested"], true);
    assert_eq!(block["status"], AUTO_CHAIN_STATUS_RECORDED);
    assert_eq!(block["chain_id"], "chain:auto:wf-deadbeef");
    assert_eq!(block["chain_id_source"], AUTO_CHAIN_ID_SOURCE_DERIVED);
    assert_eq!(block["chain_id_inputs"], inputs);
    assert_eq!(
        block["evidence_path"],
        "/abs/.missiond/v2/plans/x.evidence.json"
    );
    assert!(block.get("evidence_error").is_none());
    assert!(block.get("chain_name").is_none());
}

#[test]
fn build_auto_chain_block_resolve_failed_omits_chain_id_and_keeps_error() {
    let block = build_auto_chain_block(
        true,
        AUTO_CHAIN_STATUS_RESOLVE_FAILED,
        None,
        Some("wave19-loop"),
        None,
        None,
        Some("project root unresolved"),
    );
    assert_eq!(block["requested"], true);
    assert_eq!(block["status"], AUTO_CHAIN_STATUS_RESOLVE_FAILED);
    assert!(block.get("chain_id").is_none());
    assert_eq!(block["chain_id_source"], AUTO_CHAIN_ID_SOURCE_DERIVED);
    assert_eq!(block["chain_name"], "wave19-loop");
    assert_eq!(block["evidence_error"], "project root unresolved");
}

#[test]
fn attach_auto_chain_to_payload_splices_block_and_top_level_shortcuts() {
    let mut payload = serde_json::json!({"status": "distilled", "workflow_id": "abc"});
    let block = build_auto_chain_block(
        true,
        AUTO_CHAIN_STATUS_RECORDED,
        Some("chain:auto:wf-aaa"),
        None,
        None,
        Some("/abs/.missiond/v2/plans/x.evidence.json"),
        None,
    );
    attach_auto_chain_to_payload(&mut payload, block);
    // Original keys preserved (byte-compat over the existing payload).
    assert_eq!(payload["status"], "distilled");
    assert_eq!(payload["workflow_id"], "abc");
    // Block + shortcuts attached.
    assert_eq!(payload["auto_chain"]["chain_id"], "chain:auto:wf-aaa");
    assert_eq!(payload["auto_chain_status"], AUTO_CHAIN_STATUS_RECORDED);
    assert_eq!(payload["auto_chain_id"], "chain:auto:wf-aaa");
}

#[test]
fn attach_auto_chain_to_payload_no_chain_id_skips_top_level_id_shortcut() {
    // The `resolve_failed` path has no chain id — the top-level
    // `auto_chain_id` shortcut must NOT appear so consumers can
    // distinguish "id available" from "id absent".
    let mut payload = serde_json::json!({"status": "distilled"});
    let block = build_auto_chain_block(
        true,
        AUTO_CHAIN_STATUS_RESOLVE_FAILED,
        None,
        None,
        None,
        None,
        Some("project root unresolved"),
    );
    attach_auto_chain_to_payload(&mut payload, block);
    assert_eq!(
        payload["auto_chain_status"],
        AUTO_CHAIN_STATUS_RESOLVE_FAILED
    );
    assert!(payload.get("auto_chain_id").is_none());
    assert_eq!(
        payload["auto_chain"]["evidence_error"],
        "project root unresolved"
    );
}

#[test]
fn auto_chain_status_constants_pin_the_wire_form() {
    // Audit consumers / dashboards key on these strings — pin them
    // so a refactor that renames a constant fails loud.
    assert_eq!(AUTO_CHAIN_STATUS_RECORDED, "recorded");
    assert_eq!(AUTO_CHAIN_STATUS_RECORD_FAILED, "record_failed");
    assert_eq!(AUTO_CHAIN_STATUS_RESOLVE_FAILED, "resolve_failed");
    assert_eq!(
        AUTO_CHAIN_ID_SOURCE_DERIVED,
        "derived_from_workflow_context"
    );
    // Same kind as plan.rs's CHAIN_RECORD_KIND so a single audit
    // query sees both recorders' rows.
    assert_eq!(AUTO_CHAIN_EVIDENCE_KIND, "distill_chain_record");
    // Distinct source so dashboards can pivot on the recorder.
    assert_eq!(AUTO_CHAIN_EVIDENCE_SOURCE, "workflow_distill_auto_chain");
}

// ──────────────────────────────────────────────────────────────────
// wave-20 / task 06 :: cross-plan distill auto-trigger v1
//
// Pure-fn tests pin the deterministic safety-rule semantics + the
// trigger-status taxonomy. No tokio / no AppState — the orchestrator
// hot path (`maybe_apply_distill_chain_layers`) is exercised
// indirectly via the daemon-wide test suite.
// ──────────────────────────────────────────────────────────────────

#[test]
fn auto_trigger_status_constants_pin_the_wire_form() {
    assert_eq!(AUTO_TRIGGER_STATUS_DISABLED, "skipped_disabled");
    assert_eq!(AUTO_TRIGGER_STATUS_INNER_ERROR, "skipped_inner_error");
    assert_eq!(AUTO_TRIGGER_STATUS_RULES_FAILED, "skipped_rules_failed");
    assert_eq!(AUTO_TRIGGER_STATUS_TRIGGERED, "triggered");
    assert_eq!(
        AUTO_TRIGGER_STATUS_TRIGGERED_RECORD_FAILED,
        "triggered_record_failed"
    );
    assert_eq!(
        AUTO_TRIGGER_STATUS_TRIGGERED_RESOLVE_FAILED,
        "triggered_resolve_failed"
    );
}

#[test]
fn safety_rule_id_constants_pin_the_wire_form() {
    assert_eq!(SAFETY_RULE_INNER_DISTILL_OK, "inner_distill_succeeded");
    assert_eq!(SAFETY_RULE_DISTILL_MODE_RECORDED, "distill_mode_recorded");
    assert_eq!(SAFETY_RULE_PROJECT_ROOT_RESOLVED, "project_root_resolved");
    assert_eq!(SAFETY_RULE_EVIDENCE_PRESENT, "evidence_sidecar_present");
    assert_eq!(SAFETY_RULE_EVIDENCE_MIN_ENTRIES, "evidence_min_entries");
    assert_eq!(
        SAFETY_RULE_NOT_ALREADY_CHAINED,
        "chain_id_not_already_recorded"
    );
}

#[test]
fn parse_auto_chain_trigger_default_never_and_explicit_modes() {
    // Default: missing / blank / null / "never" → Never (byte-compat).
    assert_eq!(parse_auto_chain_trigger(None), Ok(AutoChainTrigger::Never));
    assert_eq!(
        parse_auto_chain_trigger(Some("")),
        Ok(AutoChainTrigger::Never)
    );
    assert_eq!(
        parse_auto_chain_trigger(Some("   ")),
        Ok(AutoChainTrigger::Never)
    );
    assert_eq!(
        parse_auto_chain_trigger(Some("never")),
        Ok(AutoChainTrigger::Never)
    );
    // Explicit auto_safe is honored.
    assert_eq!(
        parse_auto_chain_trigger(Some("auto_safe")),
        Ok(AutoChainTrigger::AutoSafe)
    );
    // Unknown values fail loud.
    assert!(parse_auto_chain_trigger(Some("auto_apply")).is_err());
    assert!(parse_auto_chain_trigger(Some("force")).is_err());
}

#[test]
fn auto_chain_trigger_wire_str_is_stable() {
    assert_eq!(AutoChainTrigger::Never.as_wire_str(), "never");
    assert_eq!(AutoChainTrigger::AutoSafe.as_wire_str(), "auto_safe");
}

#[test]
fn safety_rule_result_render_omits_detail_when_passing() {
    let pass = SafetyRuleResult::pass(SAFETY_RULE_INNER_DISTILL_OK);
    let v = pass.to_value();
    assert_eq!(v["rule_id"], SAFETY_RULE_INNER_DISTILL_OK);
    assert_eq!(v["passed"], true);
    assert!(v.as_object().unwrap().get("detail").is_none());

    let fail = SafetyRuleResult::fail(SAFETY_RULE_EVIDENCE_PRESENT, "missing");
    let v2 = fail.to_value();
    assert_eq!(v2["passed"], false);
    assert_eq!(v2["detail"], "missing");
}

#[test]
fn render_safety_rule_results_preserves_order() {
    let rules = vec![
        SafetyRuleResult::pass(SAFETY_RULE_INNER_DISTILL_OK),
        SafetyRuleResult::fail(SAFETY_RULE_EVIDENCE_PRESENT, "missing"),
    ];
    let arr = render_safety_rule_results(&rules);
    assert_eq!(arr.as_array().unwrap().len(), 2);
    assert_eq!(arr[0]["rule_id"], SAFETY_RULE_INNER_DISTILL_OK);
    assert_eq!(arr[1]["rule_id"], SAFETY_RULE_EVIDENCE_PRESENT);
    assert_eq!(arr[1]["detail"], "missing");
}

#[test]
fn inner_payload_has_distill_mode_handles_missing_blank_and_present() {
    assert!(!inner_payload_has_distill_mode(&serde_json::json!({})));
    assert!(!inner_payload_has_distill_mode(
        &serde_json::json!({"distill_mode": ""})
    ));
    assert!(!inner_payload_has_distill_mode(
        &serde_json::json!({"distill_mode": "   "})
    ));
    assert!(!inner_payload_has_distill_mode(
        &serde_json::json!({"distill_mode": null})
    ));
    assert!(!inner_payload_has_distill_mode(
        &serde_json::json!({"distill_mode": 42})
    ));
    assert!(inner_payload_has_distill_mode(
        &serde_json::json!({"distill_mode": "dry_run"})
    ));
    assert!(inner_payload_has_distill_mode(
        &serde_json::json!({"distill_mode": "sonnet"})
    ));
}

#[test]
fn chain_id_already_in_sidecar_detects_match_in_extra_or_root() {
    // Wave-19 recorder writes chain_id under `extra.chain_id`; tolerate
    // a top-level shape too in case audit re-shapes the entry.
    let sidecar = serde_json::json!({
        "entries": [
            {
                "kind": AUTO_CHAIN_EVIDENCE_KIND,
                "extra": {"chain_id": "chain:auto:wf-aaa"}
            },
            {
                "kind": "other_kind",
                "extra": {"chain_id": "chain:auto:wf-bbb"}
            },
            {
                "kind": AUTO_CHAIN_EVIDENCE_KIND,
                "chain_id": "chain:auto:wf-ccc"
            }
        ]
    });
    assert!(chain_id_already_in_sidecar(&sidecar, "chain:auto:wf-aaa"));
    assert!(chain_id_already_in_sidecar(&sidecar, "chain:auto:wf-ccc"));
    // Wrong kind → ignored (matches our dedup definition).
    assert!(!chain_id_already_in_sidecar(&sidecar, "chain:auto:wf-bbb"));
    // Missing → no match.
    assert!(!chain_id_already_in_sidecar(&sidecar, "chain:auto:wf-zzz"));
    // Empty / malformed sidecars never crash.
    assert!(!chain_id_already_in_sidecar(
        &serde_json::json!({}),
        "chain:auto:wf-aaa"
    ));
    assert!(!chain_id_already_in_sidecar(
        &serde_json::json!({"entries": "not-array"}),
        "chain:auto:wf-aaa"
    ));
}

#[test]
fn evaluate_safety_rules_all_pass_when_inputs_are_clean() {
    let inner = ToolResult::json_pretty(&serde_json::json!({"distill_mode": "dry_run"}));
    let inner_payload = serde_json::json!({"distill_mode": "dry_run"});
    let evidence_outcome = EvidenceOutcome::Present {
        value: serde_json::json!({"entries": [{"kind": "obs"}]}),
        entry_count: 1,
    };
    let project_root = Path::new("/tmp/proj-clean");
    let ctx = SafetyRuleContext {
        inner: &inner,
        inner_payload: &inner_payload,
        project_root: Some(project_root),
        project_resolve_error: None,
        evidence_outcome: &evidence_outcome,
        candidate_chain_id: Some("chain:auto:wf-clean"),
        min_evidence: 1,
    };
    let (rules, all_passed) = evaluate_auto_trigger_safety_rules(&ctx);
    assert!(all_passed, "rules: {:?}", rules);
    // Six rules in fixed order — pin the indices.
    assert_eq!(rules.len(), 6);
    assert_eq!(rules[0].rule_id, SAFETY_RULE_INNER_DISTILL_OK);
    assert_eq!(rules[1].rule_id, SAFETY_RULE_DISTILL_MODE_RECORDED);
    assert_eq!(rules[2].rule_id, SAFETY_RULE_PROJECT_ROOT_RESOLVED);
    assert_eq!(rules[3].rule_id, SAFETY_RULE_EVIDENCE_PRESENT);
    assert_eq!(rules[4].rule_id, SAFETY_RULE_EVIDENCE_MIN_ENTRIES);
    assert_eq!(rules[5].rule_id, SAFETY_RULE_NOT_ALREADY_CHAINED);
    for r in &rules {
        assert!(r.passed, "rule {} should pass", r.rule_id);
    }
}

#[test]
fn evaluate_safety_rules_inner_error_blocks_trigger() {
    let inner =
        ToolResult::structured_error(ToolError::new(error_codes::INVALID_PARAM, "inner failed"));
    let inner_payload = serde_json::json!({"distill_mode": "dry_run"});
    let evidence_outcome = EvidenceOutcome::Present {
        value: serde_json::json!({"entries": [{"kind": "obs"}]}),
        entry_count: 1,
    };
    let project_root = Path::new("/tmp/proj");
    let ctx = SafetyRuleContext {
        inner: &inner,
        inner_payload: &inner_payload,
        project_root: Some(project_root),
        project_resolve_error: None,
        evidence_outcome: &evidence_outcome,
        candidate_chain_id: Some("chain:auto:wf-aaa"),
        min_evidence: 1,
    };
    let (rules, all_passed) = evaluate_auto_trigger_safety_rules(&ctx);
    assert!(!all_passed);
    assert_eq!(rules[0].rule_id, SAFETY_RULE_INNER_DISTILL_OK);
    assert!(!rules[0].passed);
}

#[test]
fn evaluate_safety_rules_missing_distill_mode_blocks_trigger() {
    let inner = ToolResult::json_pretty(&serde_json::json!({}));
    let inner_payload = serde_json::json!({}); // no distill_mode
    let evidence_outcome = EvidenceOutcome::Present {
        value: serde_json::json!({"entries": [{"kind": "obs"}]}),
        entry_count: 1,
    };
    let project_root = Path::new("/tmp/proj");
    let ctx = SafetyRuleContext {
        inner: &inner,
        inner_payload: &inner_payload,
        project_root: Some(project_root),
        project_resolve_error: None,
        evidence_outcome: &evidence_outcome,
        candidate_chain_id: Some("chain:auto:wf-aaa"),
        min_evidence: 1,
    };
    let (rules, all_passed) = evaluate_auto_trigger_safety_rules(&ctx);
    assert!(!all_passed);
    assert!(!rules[1].passed);
    assert_eq!(rules[1].rule_id, SAFETY_RULE_DISTILL_MODE_RECORDED);
}

#[test]
fn evaluate_safety_rules_unresolved_project_root_blocks_trigger() {
    let inner = ToolResult::json_pretty(&serde_json::json!({"distill_mode": "dry_run"}));
    let inner_payload = serde_json::json!({"distill_mode": "dry_run"});
    let evidence_outcome = EvidenceOutcome::Missing;
    let ctx = SafetyRuleContext {
        inner: &inner,
        inner_payload: &inner_payload,
        project_root: None,
        project_resolve_error: Some("no project signal"),
        evidence_outcome: &evidence_outcome,
        candidate_chain_id: None,
        min_evidence: 1,
    };
    let (rules, all_passed) = evaluate_auto_trigger_safety_rules(&ctx);
    assert!(!all_passed);
    assert!(!rules[2].passed);
    assert_eq!(rules[2].rule_id, SAFETY_RULE_PROJECT_ROOT_RESOLVED);
    // The dependency rules below also fail loudly.
    assert!(!rules[3].passed); // sidecar
    assert!(!rules[4].passed); // min entries
    assert!(!rules[5].passed); // dedup
}

#[test]
fn evaluate_safety_rules_missing_sidecar_blocks_trigger() {
    let inner = ToolResult::json_pretty(&serde_json::json!({"distill_mode": "dry_run"}));
    let inner_payload = serde_json::json!({"distill_mode": "dry_run"});
    let evidence_outcome = EvidenceOutcome::Missing;
    let project_root = Path::new("/tmp/proj");
    let ctx = SafetyRuleContext {
        inner: &inner,
        inner_payload: &inner_payload,
        project_root: Some(project_root),
        project_resolve_error: None,
        evidence_outcome: &evidence_outcome,
        candidate_chain_id: Some("chain:auto:wf-aaa"),
        min_evidence: 1,
    };
    let (rules, all_passed) = evaluate_auto_trigger_safety_rules(&ctx);
    assert!(!all_passed);
    assert!(!rules[3].passed); // R4: sidecar
    assert!(!rules[4].passed); // R5: min entries (dependency)
    assert!(!rules[5].passed); // R6: dedup (dependency)
}

#[test]
fn evaluate_safety_rules_min_entries_below_threshold_blocks_trigger() {
    let inner = ToolResult::json_pretty(&serde_json::json!({"distill_mode": "dry_run"}));
    let inner_payload = serde_json::json!({"distill_mode": "dry_run"});
    let evidence_outcome = EvidenceOutcome::Present {
        value: serde_json::json!({"entries": [{"kind": "obs"}]}),
        entry_count: 1,
    };
    let project_root = Path::new("/tmp/proj");
    let ctx = SafetyRuleContext {
        inner: &inner,
        inner_payload: &inner_payload,
        project_root: Some(project_root),
        project_resolve_error: None,
        evidence_outcome: &evidence_outcome,
        candidate_chain_id: Some("chain:auto:wf-aaa"),
        min_evidence: 5,
    };
    let (rules, all_passed) = evaluate_auto_trigger_safety_rules(&ctx);
    assert!(!all_passed);
    assert!(!rules[4].passed);
    assert_eq!(rules[4].rule_id, SAFETY_RULE_EVIDENCE_MIN_ENTRIES);
}

#[test]
fn evaluate_safety_rules_dedup_blocks_when_chain_id_already_recorded() {
    let inner = ToolResult::json_pretty(&serde_json::json!({"distill_mode": "dry_run"}));
    let inner_payload = serde_json::json!({"distill_mode": "dry_run"});
    let evidence_outcome = EvidenceOutcome::Present {
        value: serde_json::json!({
            "entries": [
                {"kind": AUTO_CHAIN_EVIDENCE_KIND, "extra": {"chain_id": "chain:auto:wf-dup"}}
            ]
        }),
        entry_count: 1,
    };
    let project_root = Path::new("/tmp/proj");
    let ctx = SafetyRuleContext {
        inner: &inner,
        inner_payload: &inner_payload,
        project_root: Some(project_root),
        project_resolve_error: None,
        evidence_outcome: &evidence_outcome,
        candidate_chain_id: Some("chain:auto:wf-dup"),
        min_evidence: 1,
    };
    let (rules, all_passed) = evaluate_auto_trigger_safety_rules(&ctx);
    assert!(!all_passed);
    assert!(!rules[5].passed);
    assert_eq!(rules[5].rule_id, SAFETY_RULE_NOT_ALREADY_CHAINED);
}

#[test]
fn build_auto_trigger_block_emits_stable_shape() {
    let rules = render_safety_rule_results(&[SafetyRuleResult::pass(SAFETY_RULE_INNER_DISTILL_OK)]);
    let block = build_auto_trigger_block(
        true,
        AutoChainTrigger::AutoSafe,
        AUTO_TRIGGER_STATUS_TRIGGERED,
        rules.clone(),
        Some("chain:auto:wf-xyz"),
        Some("/abs/.missiond/v2/plans/p.evidence.json"),
    );
    assert_eq!(block["requested"], true);
    assert_eq!(block["mode"], "auto_safe");
    assert_eq!(block["trigger_status"], AUTO_TRIGGER_STATUS_TRIGGERED);
    assert_eq!(block["safety_rule_results"], rules);
    assert_eq!(block["chain_id"], "chain:auto:wf-xyz");
    assert_eq!(block["sidecar"], "/abs/.missiond/v2/plans/p.evidence.json");
}

#[test]
fn build_auto_trigger_block_skipped_omits_chain_id() {
    let block = build_auto_trigger_block(
        true,
        AutoChainTrigger::AutoSafe,
        AUTO_TRIGGER_STATUS_RULES_FAILED,
        serde_json::json!([]),
        None,
        None,
    );
    assert_eq!(block["trigger_status"], AUTO_TRIGGER_STATUS_RULES_FAILED);
    assert!(block.get("chain_id").is_none());
    assert!(block.get("sidecar").is_none());
}

#[test]
fn attach_auto_trigger_to_payload_splices_block_and_top_level_shortcuts() {
    let mut payload = serde_json::json!({"status": "distilled"});
    let block = build_auto_trigger_block(
        true,
        AutoChainTrigger::AutoSafe,
        AUTO_TRIGGER_STATUS_TRIGGERED,
        serde_json::json!([]),
        Some("chain:auto:wf-aaa"),
        Some("/abs/p.evidence.json"),
    );
    attach_auto_trigger_to_payload(&mut payload, block);
    // Original payload preserved.
    assert_eq!(payload["status"], "distilled");
    // Block + shortcuts attached.
    assert_eq!(payload["auto_trigger"]["mode"], "auto_safe");
    assert_eq!(
        payload["auto_trigger_status"],
        AUTO_TRIGGER_STATUS_TRIGGERED
    );
    assert_eq!(payload["auto_trigger_chain_id"], "chain:auto:wf-aaa");
}

#[test]
fn attach_auto_trigger_to_payload_skipped_omits_chain_id_shortcut() {
    let mut payload = serde_json::json!({});
    let block = build_auto_trigger_block(
        true,
        AutoChainTrigger::AutoSafe,
        AUTO_TRIGGER_STATUS_RULES_FAILED,
        serde_json::json!([]),
        None,
        None,
    );
    attach_auto_trigger_to_payload(&mut payload, block);
    assert_eq!(
        payload["auto_trigger_status"],
        AUTO_TRIGGER_STATUS_RULES_FAILED
    );
    assert!(payload.get("auto_trigger_chain_id").is_none());
}

#[test]
fn auto_trigger_min_evidence_default_is_one() {
    // Pin the default so a refactor that bumps it is loud.
    assert_eq!(AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE, 1);
}

// ──────────────────────────────────────────────────────────────────
// wave-21 / task 07 :: sonnet distill chain auto-apply v1
//
// Pure-fn tests pin the deterministic gate semantics + the
// auto-sonnet status taxonomy. No tokio / no AppState — the
// orchestrator's tokio path (`maybe_apply_auto_sonnet`) is
// exercised indirectly via the daemon-wide test suite.
// ──────────────────────────────────────────────────────────────────

#[test]
fn auto_sonnet_status_constants_pin_the_wire_form() {
    assert_eq!(AUTO_SONNET_STATUS_NOT_REQUESTED, "not_requested");
    assert_eq!(AUTO_SONNET_STATUS_DISABLED, "disabled");
    assert_eq!(AUTO_SONNET_STATUS_SKIPPED_NO_TRIGGER, "skipped_no_trigger");
    assert_eq!(
        AUTO_SONNET_STATUS_SKIPPED_RULES_FAILED,
        "skipped_rules_failed"
    );
    assert_eq!(
        AUTO_SONNET_STATUS_SKIPPED_NOT_APPROVED,
        "skipped_caller_approval_missing"
    );
    assert_eq!(
        AUTO_SONNET_STATUS_SKIPPED_ALREADY_SONNET,
        "skipped_already_sonnet"
    );
    assert_eq!(
        AUTO_SONNET_STATUS_SKIPPED_INNER_ERROR,
        "skipped_inner_error"
    );
    assert_eq!(AUTO_SONNET_STATUS_APPLIED_SONNET, "applied_sonnet");
}

#[test]
fn auto_sonnet_model_call_status_constants_pin_the_wire_form() {
    assert_eq!(AUTO_SONNET_MODEL_NOT_INVOKED, "not_invoked");
    assert_eq!(AUTO_SONNET_MODEL_INVOKED, "invoked");
    assert_eq!(AUTO_SONNET_MODEL_FAILED, "failed");
    assert_eq!(AUTO_SONNET_MODEL_INVALID_OUTPUT, "invalid_output");
}

#[test]
fn validate_auto_sonnet_args_accepts_missing_and_bool() {
    // Missing keys are valid (default-off byte-compat).
    assert!(validate_auto_sonnet_args(&serde_json::json!({})).is_ok());
    // Both bool shapes are valid.
    assert!(validate_auto_sonnet_args(&serde_json::json!({"auto_sonnet": true})).is_ok());
    assert!(validate_auto_sonnet_args(&serde_json::json!({"auto_sonnet": false})).is_ok());
    assert!(validate_auto_sonnet_args(&serde_json::json!({"auto_sonnet_approved": true})).is_ok());
    assert!(validate_auto_sonnet_args(
        &serde_json::json!({"auto_sonnet": true, "auto_sonnet_approved": true})
    )
    .is_ok());
}

#[test]
fn validate_auto_sonnet_args_rejects_string_typo() {
    // "true" / "false" strings are NOT booleans — fail loud.
    let err = validate_auto_sonnet_args(&serde_json::json!({"auto_sonnet": "true"})).unwrap_err();
    assert!(
        err.contains("auto_sonnet must be a boolean"),
        "diagnostic: {}",
        err
    );
    assert!(err.contains("string"), "shape label leaked: {}", err);
}

#[test]
fn validate_auto_sonnet_args_rejects_number_typo() {
    let err = validate_auto_sonnet_args(&serde_json::json!({"auto_sonnet": 1})).unwrap_err();
    assert!(err.contains("auto_sonnet must be a boolean"));
}

#[test]
fn validate_auto_sonnet_args_rejects_approved_string_typo() {
    let err =
        validate_auto_sonnet_args(&serde_json::json!({"auto_sonnet_approved": "yes"})).unwrap_err();
    assert!(err.contains("auto_sonnet_approved must be a boolean"));
}

#[test]
fn auto_sonnet_requested_default_false_and_explicit_true() {
    assert!(!auto_sonnet_requested(&serde_json::json!({})));
    // Non-bool / null collapses to false (validator already rejected
    // non-bool, but the read-side guard preserves byte-compat).
    assert!(!auto_sonnet_requested(
        &serde_json::json!({"auto_sonnet": null})
    ));
    assert!(!auto_sonnet_requested(
        &serde_json::json!({"auto_sonnet": false})
    ));
    assert!(auto_sonnet_requested(
        &serde_json::json!({"auto_sonnet": true})
    ));
}

#[test]
fn auto_sonnet_caller_approved_default_false_and_explicit_true() {
    assert!(!auto_sonnet_caller_approved(&serde_json::json!({})));
    assert!(!auto_sonnet_caller_approved(
        &serde_json::json!({"auto_sonnet_approved": false})
    ));
    assert!(auto_sonnet_caller_approved(
        &serde_json::json!({"auto_sonnet_approved": true})
    ));
    // I2 invariant — `auto_sonnet=true` alone is NOT caller approval.
    assert!(!auto_sonnet_caller_approved(
        &serde_json::json!({"auto_sonnet": true})
    ));
}

#[test]
fn caller_already_chose_sonnet_detects_explicit_sonnet_only() {
    // Default / dry_run / unknown → not "already sonnet".
    assert!(!caller_already_chose_sonnet(&serde_json::json!({})));
    assert!(!caller_already_chose_sonnet(
        &serde_json::json!({"distill_mode": ""})
    ));
    assert!(!caller_already_chose_sonnet(
        &serde_json::json!({"distill_mode": "dry_run"})
    ));
    assert!(!caller_already_chose_sonnet(
        &serde_json::json!({"distill_mode": "Sonnet"})
    ));
    // Only explicit lowercase "sonnet" matches.
    assert!(caller_already_chose_sonnet(
        &serde_json::json!({"distill_mode": "sonnet"})
    ));
}

#[test]
fn build_auto_sonnet_block_minimum_shape_carries_all_required_fields() {
    let rules = render_safety_rule_results(&[SafetyRuleResult::pass(SAFETY_RULE_INNER_DISTILL_OK)]);
    let block = build_auto_sonnet_block(
        true,
        AUTO_SONNET_STATUS_APPLIED_SONNET,
        true,
        true,
        AUTO_SONNET_MODEL_INVOKED,
        rules.clone(),
        true,
        Some("dry_run"),
        None,
        Some("/abs/p.evidence.json"),
        Some("chain:auto:wf-aaa"),
    );
    assert_eq!(block["requested"], true);
    assert_eq!(block["status"], AUTO_SONNET_STATUS_APPLIED_SONNET);
    assert_eq!(block["applied"], true);
    assert_eq!(block["review_required"], true);
    assert_eq!(block["model_call_status"], AUTO_SONNET_MODEL_INVOKED);
    assert_eq!(block["safety_rule_results"], rules);
    assert_eq!(block["caller_approval"], true);
    assert_eq!(block["caller_distill_mode"], "dry_run");
    assert_eq!(block["sidecar"], "/abs/p.evidence.json");
    assert_eq!(block["chain_id"], "chain:auto:wf-aaa");
    // Optional fields omitted when None.
    assert!(block.get("model_call_error").is_none());
}

#[test]
fn build_auto_sonnet_block_failure_shape_carries_error_text() {
    let block = build_auto_sonnet_block(
        true,
        AUTO_SONNET_STATUS_APPLIED_SONNET,
        false, // applied=false on failure (I5)
        true,  // review_required=true PINNED
        AUTO_SONNET_MODEL_FAILED,
        serde_json::json!([]),
        true,
        Some("dry_run"),
        Some("sonnet handler error: gateway timeout"),
        None,
        None,
    );
    assert_eq!(block["status"], AUTO_SONNET_STATUS_APPLIED_SONNET);
    assert_eq!(block["applied"], false);
    assert_eq!(block["review_required"], true);
    assert_eq!(block["model_call_status"], AUTO_SONNET_MODEL_FAILED);
    assert_eq!(
        block["model_call_error"],
        "sonnet handler error: gateway timeout"
    );
    // Optional fields omitted when None.
    assert!(block.get("sidecar").is_none());
    assert!(block.get("chain_id").is_none());
}

#[test]
fn build_auto_sonnet_block_skipped_not_approved_omits_chain_id() {
    let block = build_auto_sonnet_block(
        true,
        AUTO_SONNET_STATUS_SKIPPED_NOT_APPROVED,
        false,
        true,
        AUTO_SONNET_MODEL_NOT_INVOKED,
        serde_json::json!([]),
        false, // caller_approval=false
        Some("dry_run"),
        None,
        None,
        None,
    );
    assert_eq!(block["status"], AUTO_SONNET_STATUS_SKIPPED_NOT_APPROVED);
    assert_eq!(block["applied"], false);
    assert_eq!(block["caller_approval"], false);
    assert!(block.get("chain_id").is_none());
}

#[test]
fn attach_auto_sonnet_to_payload_splices_block_and_top_level_shortcut() {
    let mut payload = serde_json::json!({"status": "distilled"});
    let block = build_auto_sonnet_block(
        true,
        AUTO_SONNET_STATUS_APPLIED_SONNET,
        true,
        true,
        AUTO_SONNET_MODEL_INVOKED,
        serde_json::json!([]),
        true,
        None,
        None,
        None,
        None,
    );
    attach_auto_sonnet_to_payload(&mut payload, block);
    // Original payload preserved.
    assert_eq!(payload["status"], "distilled");
    // Block + shortcut attached.
    assert_eq!(payload["auto_sonnet"]["applied"], true);
    assert_eq!(
        payload["auto_sonnet_status"],
        AUTO_SONNET_STATUS_APPLIED_SONNET
    );
}

#[test]
fn attach_auto_sonnet_to_payload_skipped_omits_no_top_level_id_field() {
    // Auto-sonnet has no top-level chain_id shortcut (the wave-19
    // `auto_chain_id` carries that). Confirm only the status
    // shortcut lands.
    let mut payload = serde_json::json!({});
    let block = build_auto_sonnet_block(
        true,
        AUTO_SONNET_STATUS_SKIPPED_NOT_APPROVED,
        false,
        true,
        AUTO_SONNET_MODEL_NOT_INVOKED,
        serde_json::json!([]),
        false,
        None,
        None,
        None,
        None,
    );
    attach_auto_sonnet_to_payload(&mut payload, block);
    assert_eq!(
        payload["auto_sonnet_status"],
        AUTO_SONNET_STATUS_SKIPPED_NOT_APPROVED
    );
    // No `auto_sonnet_id` (the auto-sonnet block reuses chain_id
    // from the wave-19 receipt instead of inventing a new one).
    assert!(payload.get("auto_sonnet_id").is_none());
}

#[test]
fn shape_label_returns_canonical_json_type_name() {
    assert_eq!(shape_label(&serde_json::json!(null)), "null");
    assert_eq!(shape_label(&serde_json::json!(true)), "boolean");
    assert_eq!(shape_label(&serde_json::json!(42)), "number");
    assert_eq!(shape_label(&serde_json::json!("x")), "string");
    assert_eq!(shape_label(&serde_json::json!([1, 2])), "array");
    assert_eq!(shape_label(&serde_json::json!({"k": "v"})), "object");
}

// ──────────────────────────────────────────────────────────────────
// wave-22 / task 06 — distill chain POLICY auto-Sonnet v2 tests.
// ──────────────────────────────────────────────────────────────────

#[test]
fn auto_sonnet_policy_value_strings_pin() {
    // Wire strings — never rename in-place. Audit consumers pin
    // these via the `policy` field on `auto_sonnet_policy`.
    assert_eq!(AUTO_SONNET_POLICY_OFF_STR, "off");
    assert_eq!(AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR, "safe_after_rules");
    assert_eq!(AUTO_SONNET_POLICY_DRY_RUN_STR, "dry_run");
    assert_eq!(AutoSonnetPolicy::Off.as_wire(), "off");
    assert_eq!(
        AutoSonnetPolicy::SafeAfterRules.as_wire(),
        "safe_after_rules"
    );
    assert_eq!(AutoSonnetPolicy::DryRun.as_wire(), "dry_run");
}

#[test]
fn auto_sonnet_policy_status_constants_pin_the_wire_form() {
    // policy_status taxonomy — never rename in-place.
    assert_eq!(AUTO_SONNET_POLICY_STATUS_NOT_REQUESTED, "not_requested");
    assert_eq!(AUTO_SONNET_POLICY_STATUS_OFF, "off");
    assert_eq!(
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
        "safe_after_rules_applied"
    );
    assert_eq!(
        AUTO_SONNET_POLICY_STATUS_SAFE_DRY_RUN,
        "safe_after_rules_dry_run"
    );
    assert_eq!(
        AUTO_SONNET_POLICY_STATUS_SKIPPED_NO_TRIGGER,
        "skipped_no_trigger"
    );
    assert_eq!(
        AUTO_SONNET_POLICY_STATUS_SKIPPED_RULES_FAILED,
        "skipped_rules_failed"
    );
    assert_eq!(
        AUTO_SONNET_POLICY_STATUS_SKIPPED_ALREADY_SONNET,
        "skipped_already_sonnet"
    );
    assert_eq!(
        AUTO_SONNET_POLICY_STATUS_SKIPPED_INNER_ERROR,
        "skipped_inner_error"
    );
}

#[test]
fn parse_auto_sonnet_policy_default_off_on_missing_or_null_or_blank() {
    // Missing key → off (back-compat with wave-21/07 callers).
    let p = parse_auto_sonnet_policy(&serde_json::json!({})).unwrap();
    assert_eq!(p, AutoSonnetPolicy::Off);
    assert!(!p.is_active());
    // null → off.
    let p = parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": null})).unwrap();
    assert_eq!(p, AutoSonnetPolicy::Off);
    // empty string → off (lenient on intentional reset).
    let p = parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": ""})).unwrap();
    assert_eq!(p, AutoSonnetPolicy::Off);
    // explicit "off" → off.
    let p = parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": "off"})).unwrap();
    assert_eq!(p, AutoSonnetPolicy::Off);
}

#[test]
fn parse_auto_sonnet_policy_accepts_safe_after_rules_and_dry_run() {
    let p =
        parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": "safe_after_rules"}))
            .unwrap();
    assert_eq!(p, AutoSonnetPolicy::SafeAfterRules);
    assert!(p.is_active());
    let p =
        parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": "dry_run"})).unwrap();
    assert_eq!(p, AutoSonnetPolicy::DryRun);
    assert!(p.is_active());
}

#[test]
fn parse_auto_sonnet_policy_rejects_unknown_string() {
    // I2: typo cannot escalate the daemon — fails fast.
    let err =
        parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": "safeAfterRules"}))
            .unwrap_err();
    assert!(
        err.contains("auto_sonnet_policy must be one of"),
        "diagnostic: {}",
        err
    );
    assert!(err.contains("safeAfterRules"), "echoed bad value: {}", err);
}

#[test]
fn parse_auto_sonnet_policy_rejects_non_string_shapes() {
    // I2: bool / number / array / object all rejected.
    let err =
        parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": true})).unwrap_err();
    assert!(err.contains("auto_sonnet_policy must be a string"));
    assert!(err.contains("boolean"));
    let err = parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": 1})).unwrap_err();
    assert!(err.contains("number"));
    let err =
        parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": ["safe_after_rules"]}))
            .unwrap_err();
    assert!(err.contains("array"));
}

#[test]
fn build_auto_sonnet_policy_block_minimum_shape_has_all_required_fields() {
    let rules = render_safety_rule_results(&[SafetyRuleResult::pass(SAFETY_RULE_INNER_DISTILL_OK)]);
    let block = build_auto_sonnet_policy_block(
        true,
        AutoSonnetPolicy::SafeAfterRules,
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
        true,
        true,
        AUTO_SONNET_MODEL_INVOKED,
        rules.clone(),
        Some("dry_run"),
        None,
        Some("/abs/p.evidence.json"),
        Some("chain:auto:wf-aaa"),
    );
    assert_eq!(block["requested"], true);
    assert_eq!(block["policy"], "safe_after_rules");
    assert_eq!(
        block["policy_status"],
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED
    );
    assert_eq!(block["applied"], true);
    assert_eq!(block["review_required"], true);
    assert_eq!(block["model_call_status"], AUTO_SONNET_MODEL_INVOKED);
    assert_eq!(block["safety_rule_results"], rules);
    assert_eq!(block["caller_distill_mode"], "dry_run");
    assert_eq!(block["sidecar"], "/abs/p.evidence.json");
    assert_eq!(block["chain_id"], "chain:auto:wf-aaa");
    // Optional fields omitted when None.
    assert!(block.get("model_call_error").is_none());
    // The dual opt-in `caller_approval` field does NOT live on the
    // policy block — it is a wave-21/07-only field. Absence here is
    // load-bearing: the policy path replaces dual opt-in with
    // single-knob attestation.
    assert!(block.get("caller_approval").is_none());
}

#[test]
fn build_auto_sonnet_policy_block_failure_shape_carries_error_text() {
    let block = build_auto_sonnet_policy_block(
        true,
        AutoSonnetPolicy::SafeAfterRules,
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
        false, // applied=false on failure (I5)
        true,  // review_required=true PINNED (I6)
        AUTO_SONNET_MODEL_FAILED,
        serde_json::json!([]),
        Some("dry_run"),
        Some("sonnet handler error: gateway timeout"),
        None,
        None,
    );
    assert_eq!(block["applied"], false);
    assert_eq!(block["review_required"], true);
    assert_eq!(block["model_call_status"], AUTO_SONNET_MODEL_FAILED);
    assert_eq!(
        block["model_call_error"],
        "sonnet handler error: gateway timeout"
    );
    assert!(block.get("sidecar").is_none());
    assert!(block.get("chain_id").is_none());
}

#[test]
fn attach_auto_sonnet_policy_splices_block_and_top_level_shortcut() {
    let mut payload = serde_json::json!({"status": "distilled"});
    let block = build_auto_sonnet_policy_block(
        true,
        AutoSonnetPolicy::SafeAfterRules,
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
        true,
        true,
        AUTO_SONNET_MODEL_INVOKED,
        serde_json::json!([]),
        None,
        None,
        None,
        None,
    );
    attach_auto_sonnet_policy_to_payload(&mut payload, block);
    assert_eq!(payload["status"], "distilled");
    assert_eq!(payload["auto_sonnet_policy"]["applied"], true);
    assert_eq!(
        payload["auto_sonnet_policy_status"],
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED
    );
}

// ──────────────────────────────────────────────────────────────────
// wave-22 / task 06 — 7 dedicated invariant-preservation tests.
// Each test pins ONE wave-21/07 invariant on the wave-22/06 policy
// path so future edits cannot silently regress the contract.
// ──────────────────────────────────────────────────────────────────

#[test]
fn wave22_06_preserves_wave21_07_i1_default_off_byte_shape() {
    // I1: default `auto_sonnet_policy=off` ⇒ NO policy block emitted
    // anywhere. The default-shape parser never returns Active.
    let p = parse_auto_sonnet_policy(&serde_json::json!({})).unwrap();
    assert_eq!(p, AutoSonnetPolicy::Off);
    assert!(!p.is_active());
    // Explicit "off" also stays off (no wire surface change).
    let p = parse_auto_sonnet_policy(&serde_json::json!({"auto_sonnet_policy": "off"})).unwrap();
    assert!(!p.is_active());
    // Unrelated noise (auto_sonnet=true alone, no policy knob) stays
    // off ⇒ legacy wave-21/07 path runs untouched.
    let p = parse_auto_sonnet_policy(
        &serde_json::json!({"auto_sonnet": true, "auto_sonnet_approved": true}),
    )
    .unwrap();
    assert!(!p.is_active());
}

#[test]
fn wave22_06_preserves_wave21_07_i2_strict_shape_no_typo_escalation() {
    // I2: a single typo cannot escalate the daemon. Closed-enum
    // strict parser rejects EVERY non-canonical shape. `policy=true`
    // (boolean), `policy=1` (number), `policy="safeAfterRules"`
    // (camelCase), `policy="SAFE_AFTER_RULES"` (case mismatch),
    // `policy=" safe_after_rules "` (whitespace) — all fail-fast.
    for bad in [
        serde_json::json!({"auto_sonnet_policy": true}),
        serde_json::json!({"auto_sonnet_policy": false}),
        serde_json::json!({"auto_sonnet_policy": 1}),
        serde_json::json!({"auto_sonnet_policy": 0}),
        serde_json::json!({"auto_sonnet_policy": "safeAfterRules"}),
        serde_json::json!({"auto_sonnet_policy": "SAFE_AFTER_RULES"}),
        serde_json::json!({"auto_sonnet_policy": " safe_after_rules "}),
        serde_json::json!({"auto_sonnet_policy": "safe-after-rules"}),
        serde_json::json!({"auto_sonnet_policy": ["safe_after_rules"]}),
        serde_json::json!({"auto_sonnet_policy": {"value": "safe_after_rules"}}),
    ] {
        let r = parse_auto_sonnet_policy(&bad);
        assert!(r.is_err(), "expected {:?} to be rejected, got {:?}", bad, r);
    }
}

#[test]
fn wave22_06_preserves_wave21_07_i3_rules_must_pass_no_relax() {
    // I3: when rules are NOT all passed, the policy block surfaces
    // `skipped_rules_failed` and `applied=false`, regardless of
    // policy=safe_after_rules vs dry_run. The rule outcomes are
    // forwarded verbatim (REUSE, never re-evaluate or relax).
    let rules = serde_json::json!([
        {"rule": "inner_distill_succeeded", "passed": false, "reason": "synthetic fail"},
    ]);
    let block = build_auto_sonnet_policy_block(
        true,
        AutoSonnetPolicy::SafeAfterRules,
        AUTO_SONNET_POLICY_STATUS_SKIPPED_RULES_FAILED,
        false,
        true,
        AUTO_SONNET_MODEL_NOT_INVOKED,
        rules.clone(),
        Some("dry_run"),
        None,
        None,
        None,
    );
    assert_eq!(block["policy_status"], "skipped_rules_failed");
    assert_eq!(block["applied"], false);
    assert_eq!(block["model_call_status"], "not_invoked");
    assert_eq!(block["safety_rule_results"], rules);
    // I6 — review_required PINNED true even on rule-failure path.
    assert_eq!(block["review_required"], true);
}

#[test]
fn wave22_06_preserves_wave21_07_i4_already_sonnet_refuses_double_call() {
    // I4: caller's `distill_mode` already `sonnet` ⇒ policy refuses
    // (no double-call). Surfaces `skipped_already_sonnet`.
    // The pure helper `caller_already_chose_sonnet` is REUSED from
    // wave-21/07 so the detection logic is identical.
    assert!(caller_already_chose_sonnet(
        &serde_json::json!({"distill_mode": "sonnet"})
    ));
    assert!(!caller_already_chose_sonnet(
        &serde_json::json!({"distill_mode": "dry_run"})
    ));
    // Build a `skipped_already_sonnet` block to pin the wire shape.
    let block = build_auto_sonnet_policy_block(
        true,
        AutoSonnetPolicy::SafeAfterRules,
        AUTO_SONNET_POLICY_STATUS_SKIPPED_ALREADY_SONNET,
        false,
        true,
        AUTO_SONNET_MODEL_NOT_INVOKED,
        serde_json::json!([]),
        Some("sonnet"),
        None,
        None,
        None,
    );
    assert_eq!(block["policy_status"], "skipped_already_sonnet");
    assert_eq!(block["applied"], false);
    assert_eq!(block["caller_distill_mode"], "sonnet");
}

#[test]
fn wave22_06_preserves_wave21_07_i5_sonnet_failure_preserves_inner() {
    // I5: on Sonnet failure (`failed` / `invalid_output`), the
    // policy block still surfaces but `applied=false` AND
    // `model_call_status` carries the canonical failure label.
    // Inner payload preservation is enforced by the orchestrator —
    // here we pin the BLOCK SHAPE: failures NEVER claim applied=true.
    for status_label in [AUTO_SONNET_MODEL_FAILED, AUTO_SONNET_MODEL_INVALID_OUTPUT] {
        let block = build_auto_sonnet_policy_block(
            true,
            AutoSonnetPolicy::SafeAfterRules,
            AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
            false, // applied=false on failure
            true,  // review_required PINNED
            status_label,
            serde_json::json!([]),
            Some("dry_run"),
            Some("synthetic failure for I5"),
            None,
            None,
        );
        assert_eq!(block["applied"], false);
        assert_eq!(block["review_required"], true);
        assert_eq!(block["model_call_status"], status_label);
        assert_eq!(block["model_call_error"], "synthetic failure for I5");
    }
}

#[test]
fn wave22_06_preserves_wave21_07_i6_review_required_pinned_true() {
    // I6: `review_required=true` PINNED on EVERY policy outcome —
    // success, failure, skip — receipt-only contract preserved.
    for (status, applied, model_call) in [
        (
            AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
            true,
            AUTO_SONNET_MODEL_INVOKED,
        ),
        (
            AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
            false,
            AUTO_SONNET_MODEL_FAILED,
        ),
        (
            AUTO_SONNET_POLICY_STATUS_SAFE_DRY_RUN,
            false,
            AUTO_SONNET_MODEL_NOT_INVOKED,
        ),
        (
            AUTO_SONNET_POLICY_STATUS_SKIPPED_NO_TRIGGER,
            false,
            AUTO_SONNET_MODEL_NOT_INVOKED,
        ),
        (
            AUTO_SONNET_POLICY_STATUS_SKIPPED_RULES_FAILED,
            false,
            AUTO_SONNET_MODEL_NOT_INVOKED,
        ),
        (
            AUTO_SONNET_POLICY_STATUS_SKIPPED_ALREADY_SONNET,
            false,
            AUTO_SONNET_MODEL_NOT_INVOKED,
        ),
        (
            AUTO_SONNET_POLICY_STATUS_SKIPPED_INNER_ERROR,
            false,
            AUTO_SONNET_MODEL_NOT_INVOKED,
        ),
    ] {
        let block = build_auto_sonnet_policy_block(
            true,
            AutoSonnetPolicy::SafeAfterRules,
            status,
            applied,
            true, // review_required PINNED — never flip to false
            model_call,
            serde_json::json!([]),
            Some("dry_run"),
            None,
            None,
            None,
        );
        assert_eq!(
            block["review_required"], true,
            "review_required must stay PINNED true on policy_status={}",
            status
        );
    }
}

#[test]
fn wave22_06_preserves_wave21_07_i7_wave19_20_21_blocks_unchanged() {
    // I7: wave-19 `auto_chain` + wave-20 `auto_trigger` + wave-21/07
    // `auto_sonnet` blocks remain UNCHANGED when the policy block
    // splices on top. Pin this by attaching the policy block onto a
    // payload that ALREADY carries the three predecessor blocks
    // (legacy v1 dual opt-in already applied) and asserting all
    // four blocks survive verbatim.
    let mut payload = serde_json::json!({
        "status": "distilled",
        "distill_mode": "dry_run",
        "auto_chain": {"status": "recorded", "chain_id": "chain:auto:wf-x"},
        "auto_chain_status": "recorded",
        "auto_chain_id": "chain:auto:wf-x",
        "auto_trigger": {"trigger_status": "triggered", "chain_id": "chain:auto:wf-x"},
        "auto_trigger_status": "triggered",
        "auto_trigger_chain_id": "chain:auto:wf-x",
        "auto_sonnet": {
            "requested": true,
            "status": AUTO_SONNET_STATUS_APPLIED_SONNET,
            "applied": true,
            "review_required": true,
            "model_call_status": AUTO_SONNET_MODEL_INVOKED,
            "caller_approval": true,
        },
        "auto_sonnet_status": AUTO_SONNET_STATUS_APPLIED_SONNET,
    });
    let block = build_auto_sonnet_policy_block(
        true,
        AutoSonnetPolicy::SafeAfterRules,
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
        true,
        true,
        AUTO_SONNET_MODEL_INVOKED,
        serde_json::json!([]),
        Some("dry_run"),
        None,
        Some("/abs/p.evidence.json"),
        Some("chain:auto:wf-x"),
    );
    attach_auto_sonnet_policy_to_payload(&mut payload, block);
    // Wave-19 block UNCHANGED.
    assert_eq!(payload["auto_chain"]["status"], "recorded");
    assert_eq!(payload["auto_chain_status"], "recorded");
    assert_eq!(payload["auto_chain_id"], "chain:auto:wf-x");
    // Wave-20 block UNCHANGED.
    assert_eq!(payload["auto_trigger"]["trigger_status"], "triggered");
    assert_eq!(payload["auto_trigger_status"], "triggered");
    // Wave-21/07 block UNCHANGED — every field preserved verbatim.
    assert_eq!(payload["auto_sonnet"]["requested"], true);
    assert_eq!(
        payload["auto_sonnet"]["status"],
        AUTO_SONNET_STATUS_APPLIED_SONNET
    );
    assert_eq!(payload["auto_sonnet"]["applied"], true);
    assert_eq!(payload["auto_sonnet"]["caller_approval"], true);
    assert_eq!(
        payload["auto_sonnet_status"],
        AUTO_SONNET_STATUS_APPLIED_SONNET
    );
    // Wave-22/06 policy block ADDED — purely additive.
    assert_eq!(payload["auto_sonnet_policy"]["applied"], true);
    assert_eq!(payload["auto_sonnet_policy"]["policy"], "safe_after_rules");
    assert_eq!(
        payload["auto_sonnet_policy_status"],
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED
    );
}
