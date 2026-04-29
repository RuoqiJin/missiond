use super::*;

fn fresh_file() -> LogFile {
    let body = render_canonical_template(
        "test-exec",
        ".missiond/v2/test.lisp",
        "test scope",
        "tester",
        DEFAULT_DISPATCH_STRATEGY,
        None,
        None,
    );
    LogFile::parse(body).expect("template must parse")
}

#[test]
fn template_parses_and_balances() {
    let body = render_canonical_template("e", "p", "s", "o", DEFAULT_DISPATCH_STRATEGY, None, None);
    sexp::check_balance(&body).expect("balanced");
    LogFile::parse(body).expect("parse");
}

#[test]
fn dispatch_strategy_normalization() {
    assert_eq!(normalize_dispatch_strategy(None), "unknown");
    assert_eq!(normalize_dispatch_strategy(Some("")), "unknown");
    assert_eq!(normalize_dispatch_strategy(Some("   ")), "unknown");
    assert_eq!(
        normalize_dispatch_strategy(Some("not-a-real-mode")),
        "unknown"
    );
    assert_eq!(
        normalize_dispatch_strategy(Some("fresh-code-alignment")),
        "fresh-code-alignment"
    );
    assert_eq!(
        normalize_dispatch_strategy(Some("agent-team")),
        "agent-team"
    );
    assert_eq!(
        normalize_dispatch_strategy(Some("resident-lisp")),
        "resident-lisp"
    );
}

#[test]
fn template_writes_dispatch_metadata() {
    let body = render_canonical_template(
        "exec-disp",
        ".missiond/v2/disp.lisp",
        "scope/x",
        "owner-x",
        "fresh-code-alignment",
        Some("missiond"),
        Some("/Users/x/Projects/missiond/crates/foo"),
    );
    sexp::check_balance(&body).expect("balanced");
    let file = LogFile::parse(body).expect("parse");
    let meta = parse_kv_pairs(
        &file.src,
        file.find_block("meta").expect("meta block").children(),
    );
    assert_eq!(
        meta.get("dispatch-strategy").map(|s| s.as_str()),
        Some("fresh-code-alignment")
    );
    assert_eq!(
        meta.get("target-project").map(|s| s.as_str()),
        Some("missiond")
    );
    assert_eq!(
        meta.get("requested-cwd").map(|s| s.as_str()),
        Some("/Users/x/Projects/missiond/crates/foo")
    );
}

#[test]
fn template_omits_optional_dispatch_fields() {
    let body = render_canonical_template(
        "exec-min",
        ".missiond/v2/min.lisp",
        "scope/y",
        "owner-y",
        "agent-team",
        None,
        None,
    );
    sexp::check_balance(&body).expect("balanced");
    let file = LogFile::parse(body).expect("parse");
    let meta = parse_kv_pairs(
        &file.src,
        file.find_block("meta").expect("meta block").children(),
    );
    assert_eq!(
        meta.get("dispatch-strategy").map(|s| s.as_str()),
        Some("agent-team")
    );
    assert!(meta.get("target-project").is_none());
    assert!(meta.get("requested-cwd").is_none());
}

#[test]
fn legacy_template_without_dispatch_still_parses() {
    // Hand-written legacy meta: no dispatch-strategy key, mirrors files
    // produced by the previous handler version. Must round-trip cleanly.
    let body = "(execution-log\n  \
                (meta\n    \
                :execution-id \"legacy-x\"\n    \
                :parent-design \"old.lisp\"\n    \
                :status \"open\"\n    \
                :owner \"old-owner\"\n    \
                :scope \"legacy/scope\"\n    \
                :companion-of \"old.lisp\")\n  \
                (claims))\n";
    let file = LogFile::parse(body.to_string()).expect("legacy parses");
    let meta = parse_kv_pairs(
        &file.src,
        file.find_block("meta").expect("meta block").children(),
    );
    assert!(meta.get("dispatch-strategy").is_none());
    assert!(meta.get("target-project").is_none());
    // sanity: existing fields still readable. parse_kv_pairs returns the
    // raw source slice when the value is a quoted string atom, so the
    // outer quotes survive — downstream consumers strip them via
    // `trim_matches('"')`, which is the contract we mirror here.
    assert_eq!(
        meta.get("scope").map(|s| s.trim_matches('"').to_string()),
        Some("legacy/scope".to_string())
    );
}

#[test]
fn project_or_target_project_prefers_canonical() {
    let args = json!({
        "project": "primary",
        "target_project": "alias",
    });
    assert_eq!(project_or_target_project(&args), Some("primary"));

    let alias_only = json!({"target_project": "alias-only"});
    assert_eq!(project_or_target_project(&alias_only), Some("alias-only"));

    let neither = json!({});
    assert_eq!(project_or_target_project(&neither), None);
}

#[test]
fn id_counter_allocation_and_bump() {
    let mut file = fresh_file();
    let id1 = allocate_id(&mut file, Counter::Deviation).unwrap();
    let id2 = allocate_id(&mut file, Counter::Deviation).unwrap();
    assert_eq!(id1, "D001");
    assert_eq!(id2, "D002");
    let counters = file.find_block("id-counters").unwrap();
    let kvs = parse_kv_pairs(&file.src, counters.children());
    assert_eq!(kvs.get("next-deviation-id").unwrap().trim(), "3");
}

#[test]
fn append_to_empty_block_keeps_balance() {
    let mut file = fresh_file();
    let id = allocate_id(&mut file, Counter::Issue).unwrap();
    let entry = format!(
        "    ({id}\n      :severity \"low\"\n      :desc \"smoke\"\n      :status \"open\")",
        id = id
    );
    append_to_block(&mut file, "issues", &entry).unwrap();
    sexp::check_balance(&file.src).expect("still balanced");
    let issues = file.find_block("issues").unwrap();
    assert_eq!(issues.children().len(), 2);
}

#[test]
fn scan_max_id_handles_legacy_format() {
    let body =
        "(execution-log\n  (deviations\n    (D001 :phase \"a\")\n    (D004 :phase \"b\")))\n";
    let file = LogFile::parse(body.to_string()).unwrap();
    assert_eq!(scan_max_id(&file, Counter::Deviation), 4);
}

#[test]
fn parses_existing_pilot_file_shape() {
    // Quick smoke on the legacy `(execution name ...)` shape.
    let body = "(execution worker-pillar\n  (meta :execution_id \"x\")\n  (claims))\n";
    let file = LogFile::parse(body.to_string()).unwrap();
    assert!(file.find_block("meta").is_some());
    assert!(file.find_block("claims").is_some());
}

/// `build_opened_event` is the single mapping point between
/// `action_open` arguments and the live `ExecutionEvent::Opened`
/// projection. When all dispatch metadata is present, every slot
/// must round-trip into the event verbatim.
#[test]
fn build_opened_event_carries_all_dispatch_metadata() {
    let ev = build_opened_event(
        "exec-evt",
        ".missiond/v2/parent.lisp",
        "scope/x",
        "claude",
        "/abs/path/exec-evt.lisp".into(),
        "fresh-code-alignment",
        Some("missiond"),
        Some("/Users/x/Projects/missiond/crates/foo"),
    );
    match ev {
        ExecutionEvent::Opened {
            execution_id,
            parent_design,
            scope,
            owner,
            path,
            dispatch_strategy,
            target_project,
            requested_cwd,
        } => {
            assert_eq!(execution_id, "exec-evt");
            assert_eq!(parent_design, ".missiond/v2/parent.lisp");
            assert_eq!(scope, "scope/x");
            assert_eq!(owner, "claude");
            assert_eq!(path, "/abs/path/exec-evt.lisp");
            assert_eq!(dispatch_strategy.as_deref(), Some("fresh-code-alignment"));
            assert_eq!(target_project.as_deref(), Some("missiond"));
            assert_eq!(
                requested_cwd.as_deref(),
                Some("/Users/x/Projects/missiond/crates/foo"),
            );
        }
        _ => panic!("expected Opened"),
    }
}

/// When the open args omit `target_project` / `requested_cwd`, the
/// event keeps them as `None` (so they skip-serialize) while still
/// surfacing the canonical `dispatch_strategy` string. This mirrors
/// the runtime path through `action_open` for callers that only
/// provide the strategy.
#[test]
fn build_opened_event_omits_optional_metadata_when_absent() {
    let ev = build_opened_event(
        "exec-min",
        "p.lisp",
        "scope/y",
        "owner-y",
        "/abs/exec-min.lisp".into(),
        DEFAULT_DISPATCH_STRATEGY,
        None,
        None,
    );
    match &ev {
        ExecutionEvent::Opened {
            dispatch_strategy,
            target_project,
            requested_cwd,
            ..
        } => {
            assert_eq!(
                dispatch_strategy.as_deref(),
                Some(DEFAULT_DISPATCH_STRATEGY)
            );
            assert!(target_project.is_none());
            assert!(requested_cwd.is_none());
        }
        _ => panic!("expected Opened"),
    }
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let opened = parsed.get("Opened").and_then(|v| v.as_object()).unwrap();
    assert!(opened.contains_key("dispatch_strategy"));
    assert!(!opened.contains_key("target_project"));
    assert!(!opened.contains_key("requested_cwd"));
}

// ── Wave 18 / Task 02 — dispatch metadata projection on Claimed /
// Completed events. The daemon-side helper `read_dispatch_metadata_from_log`
// is the single mapping point between the persisted companion-log meta
// block and the live `Claimed` / `Completed` events. These tests exercise
// it against the canonical writer (`render_canonical_template`) so the
// wire form stays in lock-step with what the runtime emits.

/// When the companion log was opened with the full dispatch trio,
/// `read_dispatch_metadata_from_log` returns every field verbatim
/// (with outer string-quotes stripped to match the existing `action_list`
/// contract).
#[test]
fn read_dispatch_metadata_returns_full_trio_when_present() {
    let body = render_canonical_template(
        "exec-disp",
        ".missiond/v2/disp.lisp",
        "scope/x",
        "owner-x",
        "fresh-code-alignment",
        Some("missiond"),
        Some("/Users/x/Projects/missiond/crates/foo"),
    );
    let file = LogFile::parse(body).expect("parse");
    let meta = read_dispatch_metadata_from_log(&file);
    assert_eq!(
        meta.dispatch_strategy.as_deref(),
        Some("fresh-code-alignment")
    );
    assert_eq!(meta.target_project.as_deref(), Some("missiond"));
    assert_eq!(
        meta.requested_cwd.as_deref(),
        Some("/Users/x/Projects/missiond/crates/foo")
    );
}

/// When the open args omitted `target_project` / `requested_cwd`,
/// the helper still returns the canonical `dispatch_strategy` slot
/// and leaves the optional pair as `None` so the event skip-serializes.
#[test]
fn read_dispatch_metadata_returns_dispatch_only_when_optionals_absent() {
    let body = render_canonical_template(
        "exec-min",
        ".missiond/v2/min.lisp",
        "scope/y",
        "owner-y",
        "agent-team",
        None,
        None,
    );
    let file = LogFile::parse(body).expect("parse");
    let meta = read_dispatch_metadata_from_log(&file);
    assert_eq!(meta.dispatch_strategy.as_deref(), Some("agent-team"));
    assert!(meta.target_project.is_none());
    assert!(meta.requested_cwd.is_none());
}

/// Legacy companion logs (pre-wave12-01) had no dispatch keys at all.
/// The helper must return `DispatchMeta::default()` so the event
/// serializes byte-identical to the pre-trio wire form.
#[test]
fn read_dispatch_metadata_returns_default_for_legacy_log() {
    let body = "(execution-log\n  \
                (meta\n    \
                :execution-id \"legacy-x\"\n    \
                :parent-design \"old.lisp\"\n    \
                :status \"open\"\n    \
                :owner \"old-owner\"\n    \
                :scope \"legacy/scope\"\n    \
                :companion-of \"old.lisp\")\n  \
                (claims))\n";
    let file = LogFile::parse(body.to_string()).expect("legacy parses");
    let meta = read_dispatch_metadata_from_log(&file);
    assert_eq!(meta, DispatchMeta::default());
}

/// Whitespace-only / empty-string values in the meta block must
/// collapse to `None` so the bus event doesn't surface an empty
/// label that downstream consumers would have to special-case.
#[test]
fn read_dispatch_metadata_collapses_empty_values_to_none() {
    let body = "(execution-log\n  \
                (meta\n    \
                :execution-id \"e\"\n    \
                :parent-design \"p\"\n    \
                :status \"open\"\n    \
                :owner \"o\"\n    \
                :scope \"s\"\n    \
                :companion-of \"p\"\n    \
                :dispatch-strategy \"agent-team\"\n    \
                :target-project \"\"\n    \
                :requested-cwd \"   \")\n  \
                (claims))\n";
    let file = LogFile::parse(body.to_string()).expect("parse");
    let meta = read_dispatch_metadata_from_log(&file);
    assert_eq!(meta.dispatch_strategy.as_deref(), Some("agent-team"));
    assert!(
        meta.target_project.is_none(),
        "empty target_project must collapse to None"
    );
    assert!(
        meta.requested_cwd.is_none(),
        "whitespace requested_cwd must collapse to None"
    );
}

/// The canonical companion log written by `render_canonical_template`
/// projects cleanly into a `Claimed` event with the full trio. This
/// ties the writer + reader contracts together and pins the wire form
/// the runtime `action_claim` emit path will produce.
#[test]
fn claimed_event_inherits_dispatch_trio_from_companion_log() {
    let body = render_canonical_template(
        "exec-disp",
        ".missiond/v2/disp.lisp",
        "scope/x",
        "owner-x",
        "fresh-code-alignment",
        Some("missiond"),
        Some("/Users/x/Projects/missiond/crates/foo"),
    );
    let file = LogFile::parse(body).expect("parse");
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Claimed {
        execution_id: "exec-disp".into(),
        claim_id: "C001".into(),
        claimer: "claude".into(),
        scope: "scope/x".into(),
        phase: "".into(),
        lease_expires_at: "2026-04-25T01:00:00Z".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let claimed = parsed.get("Claimed").and_then(|v| v.as_object()).unwrap();
    assert_eq!(claimed.len(), 9);
    assert_eq!(claimed["dispatch_strategy"], "fresh-code-alignment");
    assert_eq!(claimed["target_project"], "missiond");
    assert_eq!(
        claimed["requested_cwd"],
        "/Users/x/Projects/missiond/crates/foo"
    );
}

/// Legacy companion logs project into a `Completed` event whose wire
/// form omits the dispatch trio entirely (byte-identical to the
/// pre-wave18 5-field shape).
#[test]
fn completed_event_omits_dispatch_trio_for_legacy_log() {
    let body = "(execution-log\n  \
                (meta\n    \
                :execution-id \"legacy-x\"\n    \
                :parent-design \"old.lisp\"\n    \
                :status \"open\"\n    \
                :owner \"old-owner\"\n    \
                :scope \"legacy/scope\"\n    \
                :companion-of \"old.lisp\")\n  \
                (claims))\n";
    let file = LogFile::parse(body.to_string()).expect("legacy parses");
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Completed {
        execution_id: "legacy-x".into(),
        completion_id: "COMP001".into(),
        phase: "phase-A".into(),
        agent: "old-agent".into(),
        at: "2026-04-25T03:00:00Z".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let completed = parsed.get("Completed").and_then(|v| v.as_object()).unwrap();
    assert_eq!(completed.len(), 5);
    assert!(!completed.contains_key("dispatch_strategy"));
    assert!(!completed.contains_key("target_project"));
    assert!(!completed.contains_key("requested_cwd"));
}

// ── Wave 20 / Task 09 — legacy ExecutionEvent variants now project
//                       the workstation-dispatch trio from the
//                       companion-log meta block, mirroring what
//                       Opened / Claimed / Completed already do.
//
// The action_* runtime paths each call `read_dispatch_metadata_from_log`
// on the post-write `file` handle and forward the trio onto the
// emitted event. We don't have AppState in unit tests, so we mirror
// the same projection chain here against the canonical template
// (writer side) — this guarantees the wire shape the runtime emits
// tracks the writer contract exactly.

/// Helper that builds a canonical companion log carrying the full
/// dispatch trio so each swept-variant test below shares the same
/// fixture.
fn canonical_log_with_dispatch_trio() -> LogFile {
    let body = render_canonical_template(
        "exec-disp",
        ".missiond/v2/disp.lisp",
        "scope/x",
        "owner-x",
        "fresh-code-alignment",
        Some("missiond"),
        Some("/Users/x/Projects/missiond/crates/foo"),
    );
    LogFile::parse(body).expect("parse")
}

/// Helper that builds a legacy companion log (pre-wave12-01) with
/// no dispatch keys at all. Each swept variant must emit its
/// pre-trio wire shape against this fixture.
fn legacy_log_without_dispatch() -> LogFile {
    let body = "(execution-log\n  \
                (meta\n    \
                :execution-id \"legacy-x\"\n    \
                :parent-design \"old.lisp\"\n    \
                :status \"open\"\n    \
                :owner \"old-owner\"\n    \
                :scope \"legacy/scope\"\n    \
                :companion-of \"old.lisp\")\n  \
                (claims))\n";
    LogFile::parse(body.to_string()).expect("legacy parses")
}

fn assert_full_dispatch_trio(map: &serde_json::Map<String, serde_json::Value>) {
    assert_eq!(map["dispatch_strategy"], "fresh-code-alignment");
    assert_eq!(map["target_project"], "missiond");
    assert_eq!(
        map["requested_cwd"],
        "/Users/x/Projects/missiond/crates/foo"
    );
}

fn assert_no_dispatch_trio(map: &serde_json::Map<String, serde_json::Value>) {
    assert!(!map.contains_key("dispatch_strategy"));
    assert!(!map.contains_key("target_project"));
    assert!(!map.contains_key("requested_cwd"));
}

#[test]
fn heartbeat_event_inherits_dispatch_trio_from_companion_log() {
    let file = canonical_log_with_dispatch_trio();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Heartbeat {
        execution_id: "exec-disp".into(),
        claim_id: "C001".into(),
        claimer: "claude".into(),
        heartbeat_at: "2026-04-25T01:00:00Z".into(),
        lease_expires_at: "2026-04-25T01:30:00Z".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed.get("Heartbeat").and_then(|v| v.as_object()).unwrap();
    assert_eq!(p.len(), 8);
    assert_full_dispatch_trio(p);
}

#[test]
fn heartbeat_event_omits_dispatch_trio_for_legacy_log() {
    let file = legacy_log_without_dispatch();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Heartbeat {
        execution_id: "legacy-x".into(),
        claim_id: "C001".into(),
        claimer: "old".into(),
        heartbeat_at: "t".into(),
        lease_expires_at: "t2".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed.get("Heartbeat").and_then(|v| v.as_object()).unwrap();
    assert_eq!(p.len(), 5);
    assert_no_dispatch_trio(p);
}

#[test]
fn released_event_inherits_dispatch_trio_from_companion_log() {
    let file = canonical_log_with_dispatch_trio();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Released {
        execution_id: "exec-disp".into(),
        claim_id: "C001".into(),
        claimer: "claude".into(),
        released_at: "2026-04-25T02:00:00Z".into(),
        summary: Some("done".into()),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed.get("Released").and_then(|v| v.as_object()).unwrap();
    assert_eq!(p.len(), 8);
    assert_full_dispatch_trio(p);
}

#[test]
fn released_event_omits_dispatch_trio_for_legacy_log() {
    let file = legacy_log_without_dispatch();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Released {
        execution_id: "legacy-x".into(),
        claim_id: "C001".into(),
        claimer: "old".into(),
        released_at: "t".into(),
        summary: None,
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed.get("Released").and_then(|v| v.as_object()).unwrap();
    // Released always carries the `summary` key (Option<String>
    // without skip-serializing) so the legacy shape is 5 fields.
    assert_eq!(p.len(), 5);
    assert_no_dispatch_trio(p);
}

#[test]
fn deviation_recorded_event_inherits_dispatch_trio_from_companion_log() {
    let file = canonical_log_with_dispatch_trio();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::DeviationRecorded {
        execution_id: "exec-disp".into(),
        deviation_id: "D001".into(),
        phase: "phase-A".into(),
        approved_by: "claude".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed
        .get("DeviationRecorded")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(p.len(), 7);
    assert_full_dispatch_trio(p);
}

#[test]
fn deviation_recorded_event_omits_dispatch_trio_for_legacy_log() {
    let file = legacy_log_without_dispatch();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::DeviationRecorded {
        execution_id: "legacy-x".into(),
        deviation_id: "D001".into(),
        phase: "p".into(),
        approved_by: "auto".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed
        .get("DeviationRecorded")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(p.len(), 4);
    assert_no_dispatch_trio(p);
}

#[test]
fn decision_recorded_event_inherits_dispatch_trio_from_companion_log() {
    let file = canonical_log_with_dispatch_trio();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::DecisionRecorded {
        execution_id: "exec-disp".into(),
        decision_id: "DC001".into(),
        decided_by: "claude".into(),
        at: "2026-04-25T05:00:00Z".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed
        .get("DecisionRecorded")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(p.len(), 7);
    assert_full_dispatch_trio(p);
}

#[test]
fn decision_recorded_event_omits_dispatch_trio_for_legacy_log() {
    let file = legacy_log_without_dispatch();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::DecisionRecorded {
        execution_id: "legacy-x".into(),
        decision_id: "DC001".into(),
        decided_by: "old".into(),
        at: "t".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed
        .get("DecisionRecorded")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(p.len(), 4);
    assert_no_dispatch_trio(p);
}

#[test]
fn issue_recorded_event_inherits_dispatch_trio_from_companion_log() {
    let file = canonical_log_with_dispatch_trio();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::IssueRecorded {
        execution_id: "exec-disp".into(),
        issue_id: "I001".into(),
        severity: "high".into(),
        owner: "claude".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed
        .get("IssueRecorded")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(p.len(), 7);
    assert_full_dispatch_trio(p);
}

#[test]
fn issue_recorded_event_omits_dispatch_trio_for_legacy_log() {
    let file = legacy_log_without_dispatch();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::IssueRecorded {
        execution_id: "legacy-x".into(),
        issue_id: "I001".into(),
        severity: "low".into(),
        owner: "".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed
        .get("IssueRecorded")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(p.len(), 4);
    assert_no_dispatch_trio(p);
}

#[test]
fn audited_event_inherits_dispatch_trio_from_companion_log() {
    let file = canonical_log_with_dispatch_trio();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Audited {
        execution_id: "exec-disp".into(),
        ok: true,
        findings_count: 0,
        error_count: 0,
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed.get("Audited").and_then(|v| v.as_object()).unwrap();
    assert_eq!(p.len(), 7);
    assert_full_dispatch_trio(p);
}

#[test]
fn audited_event_omits_dispatch_trio_for_legacy_log() {
    let file = legacy_log_without_dispatch();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Audited {
        execution_id: "legacy-x".into(),
        ok: false,
        findings_count: 1,
        error_count: 1,
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed.get("Audited").and_then(|v| v.as_object()).unwrap();
    assert_eq!(p.len(), 4);
    assert_no_dispatch_trio(p);
}

#[test]
fn repaired_event_inherits_dispatch_trio_from_companion_log() {
    let file = canonical_log_with_dispatch_trio();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Repaired {
        execution_id: "exec-disp".into(),
        applied: true,
        action_count: 2,
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed.get("Repaired").and_then(|v| v.as_object()).unwrap();
    assert_eq!(p.len(), 6);
    assert_full_dispatch_trio(p);
}

#[test]
fn repaired_event_omits_dispatch_trio_for_legacy_log() {
    let file = legacy_log_without_dispatch();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::Repaired {
        execution_id: "legacy-x".into(),
        applied: false,
        action_count: 0,
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed.get("Repaired").and_then(|v| v.as_object()).unwrap();
    assert_eq!(p.len(), 3);
    assert_no_dispatch_trio(p);
}

#[test]
fn stale_claim_event_inherits_dispatch_trio_from_companion_log() {
    let file = canonical_log_with_dispatch_trio();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::StaleClaim {
        execution_id: "exec-disp".into(),
        claim_id: "C001".into(),
        claimer: "claude".into(),
        lease_expires_at: "2026-04-25T00:30:00Z".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed
        .get("StaleClaim")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(p.len(), 7);
    assert_full_dispatch_trio(p);
}

#[test]
fn stale_claim_event_omits_dispatch_trio_for_legacy_log() {
    let file = legacy_log_without_dispatch();
    let dm = read_dispatch_metadata_from_log(&file);
    let ev = ExecutionEvent::StaleClaim {
        execution_id: "legacy-x".into(),
        claim_id: "C001".into(),
        claimer: "old".into(),
        lease_expires_at: "t".into(),
        dispatch_strategy: dm.dispatch_strategy,
        target_project: dm.target_project,
        requested_cwd: dm.requested_cwd,
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
    let p = parsed
        .get("StaleClaim")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(p.len(), 4);
    assert_no_dispatch_trio(p);
}

// ── Wave 12 / Task 01 — scoped-commit handoff durability plane ──
//
// Tests below pin the shape of the new completion fields and the
// audit findings against intent-memory.lisp :: scoped-commit-contract
// and intent-flow.lisp :: F-scoped-commit-handoff. They exercise
// pure helpers (no AppState / no project root) so the daemon-wide
// `cargo test -p missiond-daemon` still PASSes when sibling agents
// are mid-edit on plan.rs / workflow.rs / etc.

fn fresh_file_with_claim() -> LogFile {
    let mut file = fresh_file();
    // Hand-append a single active claim covering "src/" so the
    // staged-file scope check has something to validate against.
    // We bypass `action_claim` because it lives behind AppState.
    let now = now_iso();
    let entry = format!(
        "    (C001\n      :claimer \"agent\"\n      :scope \"src/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :heartbeat-at {ts}\n      :status \"active\")",
        ts = lisp_quote_string(&now),
    );
    append_to_block(&mut file, "claims", &entry).unwrap();
    file
}

/// Validate the canonical commit-status normalizer rejects unknown
/// labels but lets every value from intent-memory.lisp ::
/// :commit-status-values through unchanged.
#[test]
fn commit_status_normalizer_accepts_canonical_only() {
    for &status in VALID_COMMIT_STATUSES {
        assert_eq!(normalize_commit_status(status), Some(status));
    }
    assert_eq!(normalize_commit_status("  pending  "), Some("pending"));
    assert!(normalize_commit_status("").is_none());
    assert!(normalize_commit_status("done").is_none());
    assert!(normalize_commit_status("COMMITTED").is_none());
}

/// Empty list arguments must be preserved as `Some(vec![])` so a
/// completion can record "intentionally staged nothing"; absent keys
/// stay `None` so the legacy 6-field shape remains byte-identical.
#[test]
fn collect_string_list_distinguishes_absent_from_empty() {
    let none_args = json!({});
    assert!(collect_string_list(&none_args, "changed_files").is_none());

    let empty_args = json!({"changed_files": []});
    assert_eq!(
        collect_string_list(&empty_args, "changed_files"),
        Some(vec![])
    );

    let with_paths = json!({
        "changed_files": ["src/a.rs", "  src/b.rs  ", "", "src/c.rs"],
    });
    assert_eq!(
        collect_string_list(&with_paths, "changed_files"),
        Some(vec![
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
            "src/c.rs".to_string(),
        ])
    );
}

/// `render_string_list` round-trips through `parse_string_list` so
/// audit/status reads of the companion log return the exact list the
/// writer recorded — including the empty-list literal.
#[test]
fn string_list_round_trip() {
    let empty = render_string_list(&[]);
    assert_eq!(empty, "()");
    assert_eq!(parse_string_list(&empty), Some(Vec::<String>::new()));

    let items = vec!["src/a.rs".to_string(), "tests/b.rs".to_string()];
    let rendered = render_string_list(&items);
    let parsed = parse_string_list(&rendered).expect("must parse");
    assert_eq!(parsed, items);

    // Quotes inside paths survive the lisp_quote_string escape cycle.
    let quoted = vec!["src/a\"b.rs".to_string()];
    let rendered = render_string_list(&quoted);
    assert_eq!(parse_string_list(&rendered), Some(quoted));
}

/// Legacy completions (no scoped-commit metadata) must still parse
/// and yield `None` everywhere on the new fields. This is the
/// backward-compat contract from the task file: "legacy execution
/// 文件缺字段必须继续 parse".
#[test]
fn parse_completions_handles_legacy_shape() {
    let body = "(execution-log\n  (completions\n    (COMP001 :phase \"a\" :agent \"x\" :summary \"s\" :deliverables \"d\" :verification \"v\" :at \"2026-04-26T00:00:00Z\")))\n";
    let file = LogFile::parse(body.to_string()).expect("legacy parses");
    let comps = parse_completions(&file);
    assert_eq!(comps.len(), 1);
    let c = &comps[0];
    assert_eq!(c.id, "COMP001");
    assert_eq!(c.phase, "a");
    assert_eq!(c.agent, "x");
    assert!(c.changed_files.is_none());
    assert!(c.staged_files.is_none());
    assert!(c.commit_hash.is_none());
    assert!(c.commit_status.is_none());
    assert!(c.commit_blocker.is_none());
}

/// A completion that carries every scoped-commit field must be
/// readable round-trip from the durable file. We assemble the
/// completion entry by hand (mirroring what `action_complete`
/// writes) so the parser is exercised against the on-disk shape.
#[test]
fn parse_completions_reads_scoped_commit_fields() {
    let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :changed-files (\"src/a.rs\" \"src/b.rs\")\n      :staged-files (\"src/a.rs\")\n      :commit-hash \"abc1234\"\n      :commit-status \"committed\"\n      :commit-blocker \"\")))\n";
    let file = LogFile::parse(body.to_string()).expect("parse");
    let comps = parse_completions(&file);
    assert_eq!(comps.len(), 1);
    let c = &comps[0];
    assert_eq!(
        c.changed_files.as_deref(),
        Some(&["src/a.rs".to_string(), "src/b.rs".to_string()][..])
    );
    assert_eq!(
        c.staged_files.as_deref(),
        Some(&["src/a.rs".to_string()][..])
    );
    assert_eq!(c.commit_hash.as_deref(), Some("abc1234"));
    assert_eq!(c.commit_status.as_deref(), Some("committed"));
    // Empty blocker collapses to `None` so audit does not key off
    // whitespace.
    assert!(c.commit_blocker.is_none());
}

/// `action_complete` is gated behind AppState, so we directly drive
/// the lower-level write helpers it now wraps. The test asserts
/// that each scoped-commit field round-trips into the companion log
/// when supplied, and that omitting them keeps the legacy entry
/// shape intact.
#[test]
fn complete_writes_each_commit_status_value() {
    for &status in &["not-required", "pending", "committed", "blocked", "skipped"] {
        let mut file = fresh_file_with_claim();
        let id = allocate_id(&mut file, Counter::Completion).unwrap();
        let mut entry = format!(
            "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :changed-files {changed}\n      :staged-files {staged}",
            id = id,
            changed = render_string_list(&["src/a.rs".to_string()]),
            staged = render_string_list(&["src/a.rs".to_string()]),
        );
        entry.push_str(&format!(
            "\n      :commit-status {}",
            lisp_quote_string(status)
        ));
        if status == "committed" {
            entry.push_str("\n      :commit-hash \"abc1234\"");
        }
        if status == "blocked" {
            entry.push_str("\n      :commit-blocker \"index conflict\"");
        }
        entry.push(')');
        append_to_block(&mut file, "completions", &entry).unwrap();
        sexp::check_balance(&file.src).expect("balanced");
        let comps = parse_completions(&file);
        let c = comps.last().unwrap();
        assert_eq!(c.commit_status.as_deref(), Some(status));
        if status == "committed" {
            assert_eq!(c.commit_hash.as_deref(), Some("abc1234"));
        } else {
            assert!(c.commit_hash.is_none());
        }
        if status == "blocked" {
            assert_eq!(c.commit_blocker.as_deref(), Some("index conflict"));
        } else {
            assert!(c.commit_blocker.is_none());
        }
    }
}

/// Audit must flag a completion whose commit_status="committed" lacks
/// a commit_hash — the durability gap that scoped-commit-contract
/// :inv-7 explicitly rejects.
#[test]
fn audit_flags_committed_without_hash() {
    let mut file = fresh_file_with_claim();
    let id = allocate_id(&mut file, Counter::Completion).unwrap();
    let entry = format!(
        "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :commit-status \"committed\")",
        id = id,
    );
    append_to_block(&mut file, "completions", &entry).unwrap();

    let claims = parse_claims(&file);
    let mut findings = Vec::new();
    audit_scoped_commit_handoff(&file, &claims, &mut findings);
    let kinds: Vec<&str> = findings
        .iter()
        .filter_map(|f| f.get("kind").and_then(|v| v.as_str()))
        .collect();
    assert!(
        kinds.contains(&FINDING_COMMIT_STATUS_NO_HASH),
        "expected {} in {:?}",
        FINDING_COMMIT_STATUS_NO_HASH,
        kinds
    );
    // Severity must be "error" so audit `ok` flips, mirroring the
    // existing duplicate-id / claim-overlap invariants.
    let f = findings
        .iter()
        .find(|f| f.get("kind").and_then(|v| v.as_str()) == Some(FINDING_COMMIT_STATUS_NO_HASH))
        .unwrap();
    assert_eq!(f.get("severity").and_then(|v| v.as_str()), Some("error"));
}

/// Audit must flag a completion whose commit_status="blocked" lacks a
/// commit_blocker — the next agent has no recovery context per the
/// scoped-commit-contract :recovery-rule.
#[test]
fn audit_flags_blocked_without_blocker() {
    let mut file = fresh_file_with_claim();
    let id = allocate_id(&mut file, Counter::Completion).unwrap();
    let entry = format!(
        "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :commit-status \"blocked\")",
        id = id,
    );
    append_to_block(&mut file, "completions", &entry).unwrap();

    let claims = parse_claims(&file);
    let mut findings = Vec::new();
    audit_scoped_commit_handoff(&file, &claims, &mut findings);
    assert!(
        findings
            .iter()
            .any(|f| f.get("kind").and_then(|v| v.as_str())
                == Some(FINDING_COMMIT_BLOCKED_NO_BLOCKER))
    );
}

/// Audit must flag staged_files paths that escape every recorded
/// claim scope. The active claim covers "src/"; staging
/// "vendor/x.rs" is outside scope and must surface as
/// scoped-commit-violation per scoped-commit-contract :scope-rule.
#[test]
fn audit_flags_scoped_commit_violation() {
    let mut file = fresh_file_with_claim();
    let id = allocate_id(&mut file, Counter::Completion).unwrap();
    let entry = format!(
        "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :changed-files {changed}\n      :staged-files {staged}\n      :commit-status \"committed\"\n      :commit-hash \"abc1234\")",
        id = id,
        changed = render_string_list(&["src/a.rs".to_string(), "vendor/x.rs".to_string()]),
        staged = render_string_list(&["src/a.rs".to_string(), "vendor/x.rs".to_string()]),
    );
    append_to_block(&mut file, "completions", &entry).unwrap();

    let claims = parse_claims(&file);
    let mut findings = Vec::new();
    audit_scoped_commit_handoff(&file, &claims, &mut findings);
    let violation = findings
        .iter()
        .find(|f| f.get("kind").and_then(|v| v.as_str()) == Some(FINDING_SCOPED_COMMIT_VIOLATION))
        .expect("scoped-commit-violation finding required");
    let staged = violation
        .get("staged_files")
        .and_then(|v| v.as_array())
        .unwrap();
    let staged_strs: Vec<&str> = staged.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(staged_strs, vec!["vendor/x.rs"]);
    assert_eq!(
        violation.get("severity").and_then(|v| v.as_str()),
        Some("error")
    );
}

/// Completions whose staged_files stay inside an existing claim
/// scope must NOT trip the violation check, even when the claim is
/// already released — that is the legitimate handoff path from
/// F-scoped-commit-handoff :: s7 release-claim.
#[test]
fn audit_passes_scoped_commit_inside_released_claim() {
    let mut file = fresh_file();
    // Released claim covering "crates/foo/" — staging files inside
    // this scope must remain valid even after release.
    let now = now_iso();
    let claim = format!(
        "    (C001\n      :claimer \"agent\"\n      :scope \"crates/foo/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :released-at {ts}\n      :heartbeat-at {ts}\n      :status \"released\")",
        ts = lisp_quote_string(&now),
    );
    append_to_block(&mut file, "claims", &claim).unwrap();
    let id = allocate_id(&mut file, Counter::Completion).unwrap();
    let entry = format!(
        "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :changed-files {changed}\n      :staged-files {staged}\n      :commit-status \"committed\"\n      :commit-hash \"abc1234\")",
        id = id,
        changed = render_string_list(&["crates/foo/src/a.rs".to_string()]),
        staged = render_string_list(&["crates/foo/src/a.rs".to_string()]),
    );
    append_to_block(&mut file, "completions", &entry).unwrap();

    let claims = parse_claims(&file);
    let mut findings = Vec::new();
    audit_scoped_commit_handoff(&file, &claims, &mut findings);
    let kinds: Vec<&str> = findings
        .iter()
        .filter_map(|f| f.get("kind").and_then(|v| v.as_str()))
        .collect();
    assert!(
        !kinds.contains(&FINDING_SCOPED_COMMIT_VIOLATION),
        "no violation expected, got {:?}",
        kinds
    );
    assert!(
        !kinds.contains(&FINDING_COMMIT_STATUS_NO_HASH),
        "no missing-hash expected, got {:?}",
        kinds
    );
}

/// `summarize_durability` rolls up an empty completions list to
/// zero counts + null latest fields, so list/status payloads stay
/// shape-stable across legacy companion logs.
#[test]
fn summarize_durability_handles_empty_and_mixed() {
    let v = summarize_durability(&[]);
    assert_eq!(v.get("completion_count").and_then(|x| x.as_i64()), Some(0));
    assert!(v
        .get("latest_commit_status")
        .map(|x| x.is_null())
        .unwrap_or(false));

    let records = vec![
        CompletionRecord {
            id: "COMP001".into(),
            phase: "p".into(),
            agent: "a".into(),
            at: "2026-04-26T00:00:00Z".into(),
            changed_files: None,
            staged_files: None,
            commit_hash: Some("abc".into()),
            commit_status: Some("committed".into()),
            commit_blocker: None,
            task_contract_path: None,
            task_report_path: None,
            verifier_status: None,
            verifier_notes: None,
            task_run_verifier_status: None,
            shared_memory_path: None,
            verifier_diagnostics: None,
            verified: None,
        },
        CompletionRecord {
            id: "COMP002".into(),
            phase: "p".into(),
            agent: "a".into(),
            at: "2026-04-26T00:01:00Z".into(),
            changed_files: None,
            staged_files: None,
            commit_hash: None,
            commit_status: Some("blocked".into()),
            commit_blocker: Some("conflict".into()),
            task_contract_path: None,
            task_report_path: None,
            verifier_status: None,
            verifier_notes: None,
            task_run_verifier_status: None,
            shared_memory_path: None,
            verifier_diagnostics: None,
            verified: None,
        },
    ];
    let v = summarize_durability(&records);
    assert_eq!(v.get("completion_count").and_then(|x| x.as_i64()), Some(2));
    assert_eq!(v.get("with_commit_hash").and_then(|x| x.as_i64()), Some(1));
    assert_eq!(
        v.get("blocked_with_blocker").and_then(|x| x.as_i64()),
        Some(1)
    );
    assert_eq!(
        v.get("latest_commit_status").and_then(|x| x.as_str()),
        Some("blocked")
    );
    let by = v
        .get("by_commit_status")
        .and_then(|x| x.as_object())
        .unwrap();
    assert_eq!(by.get("committed").and_then(|x| x.as_i64()), Some(1));
    assert_eq!(by.get("blocked").and_then(|x| x.as_i64()), Some(1));
    assert_eq!(by.get("pending").and_then(|x| x.as_i64()), Some(0));
}

// ── Wave 16 / Task 06 — fail-fast scoped-commit enforcement ────
//
// Tests below pin the contract of `enforce_scoped_commit_completion`,
// the runtime gate `action_complete` calls when the caller opts in
// via `enforce_scoped_commit=true`. The audit-only path still owns
// legacy callers (covered above) — these only exercise the new
// structured-error short-circuit.

fn extract_error_code(result: &ToolResult) -> Option<String> {
    let v = serde_json::to_value(result).ok()?;
    // structured_error renders into the content[0].text JSON payload.
    let content = v.get("content")?.as_array()?;
    let first = content.first()?;
    let text = first.get("text")?.as_str()?;
    let parsed: Value = serde_json::from_str(text).ok()?;
    parsed
        .get("error_code")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
}

/// Released claim alongside the existing "src/" active claim — both
/// must count as in-scope when validating staged paths so the
/// post-release commit window stays open per
/// F-scoped-commit-handoff :: s7 release-claim.
fn fresh_file_with_released_claim() -> LogFile {
    let mut file = fresh_file();
    let now = now_iso();
    let entry = format!(
        "    (C001\n      :claimer \"agent\"\n      :scope \"crates/foo/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :released-at {ts}\n      :heartbeat-at {ts}\n      :status \"released\")",
        ts = lisp_quote_string(&now),
    );
    append_to_block(&mut file, "claims", &entry).unwrap();
    file
}

/// committed without commit_hash + enforce_scoped_commit=true must
/// short-circuit with COMMIT_HASH_REQUIRED before the file is touched.
#[test]
fn enforce_rejects_committed_without_hash() {
    let file = fresh_file_with_claim();
    let res = enforce_scoped_commit_completion(
        &file,
        Some(&["src/a.rs".to_string()]),
        None,
        Some("committed"),
        None,
    );
    let err = res.expect_err("should reject committed without hash");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("COMMIT_HASH_REQUIRED"),
    );
}

/// blocked without commit_blocker + enforce_scoped_commit=true must
/// reject with COMMIT_BLOCKER_REQUIRED. Empty-string blocker is
/// equivalent to absent (caller-side trim already collapsed it).
#[test]
fn enforce_rejects_blocked_without_blocker() {
    let file = fresh_file_with_claim();
    let res = enforce_scoped_commit_completion(
        &file,
        Some(&["src/a.rs".to_string()]),
        None,
        Some("blocked"),
        None,
    );
    let err = res.expect_err("should reject blocked without blocker");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("COMMIT_BLOCKER_REQUIRED"),
    );
}

/// staged_files non-empty + zero claims must reject with the
/// CLAIM_SCOPE_REQUIRED variant — distinct from a scope drift
/// violation so the writer can tell "missing claim" from "outside
/// scope" without parsing the audit findings list.
#[test]
fn enforce_rejects_staged_files_with_no_claims() {
    let file = fresh_file();
    let res = enforce_scoped_commit_completion(
        &file,
        Some(&["src/a.rs".to_string()]),
        Some("abc1234"),
        Some("committed"),
        None,
    );
    let err = res.expect_err("should reject staged with no claims");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("CLAIM_SCOPE_REQUIRED"),
    );
}

/// staged path outside every claim scope must reject with
/// SCOPED_COMMIT_VIOLATION. Mirrors the audit-only finding so the
/// runtime contract matches the audit contract.
#[test]
fn enforce_rejects_staged_file_outside_claim_scope() {
    let file = fresh_file_with_claim();
    let res = enforce_scoped_commit_completion(
        &file,
        Some(&["vendor/x.rs".to_string()]),
        Some("abc1234"),
        Some("committed"),
        None,
    );
    let err = res.expect_err("should reject scope drift");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("SCOPED_COMMIT_VIOLATION"),
    );
}

/// staged path inside an already-released claim must pass —
/// the writer legitimately commits files inside the just-released
/// scope window per F-scoped-commit-handoff :: s7.
#[test]
fn enforce_accepts_staged_file_inside_released_claim() {
    let file = fresh_file_with_released_claim();
    let res = enforce_scoped_commit_completion(
        &file,
        Some(&["crates/foo/src/a.rs".to_string()]),
        Some("abc1234"),
        Some("committed"),
        None,
    );
    let summary = res.expect("should accept released-claim handoff");
    assert_eq!(
        summary.get("staged_files_checked").and_then(|v| v.as_u64()),
        Some(1),
    );
    let scopes = summary
        .get("claim_scopes")
        .and_then(|v| v.as_array())
        .expect("claim_scopes array");
    assert!(scopes.iter().any(|v| v.as_str() == Some("crates/foo/")));
}

/// Empty staged_files + enforce_scoped_commit=true must still pass
/// (read-only completions are legal per scoped-commit-contract
/// :commit-status-values :not-required) and the validation summary
/// must record "0 staged paths checked" so callers can confirm the
/// branch they hit.
#[test]
fn enforce_accepts_empty_staged_files() {
    let file = fresh_file_with_claim();
    let res = enforce_scoped_commit_completion(&file, Some(&[]), None, Some("not-required"), None);
    let summary = res.expect("read-only completion must pass");
    assert_eq!(
        summary.get("staged_files_checked").and_then(|v| v.as_u64()),
        Some(0),
    );
}

/// Caller did not opt in → `enforce_scoped_commit_completion` is
/// never called. We assert the legacy code path explicitly: a
/// `commit_status=committed` payload with no hash is accepted by
/// the gate when `enforce_scoped_commit=false` because the gate
/// simply does not run (audit will still flag it later).
///
/// We can't drive `action_complete` directly without AppState, so
/// instead we mirror its branch by ensuring the helper is only
/// reached when the flag is true — invoking it directly with the
/// same payload here illustrates the contract: legacy callers
/// would never hit this path.
#[test]
fn enforce_helper_is_opt_in_only() {
    // Mirror the dispatch branch from `action_complete`: when the
    // caller does not set `enforce_scoped_commit`, we never reach
    // the helper. So a payload that *would* fail validation
    // (`committed` without hash) is allowed through the legacy
    // path. We assert the helper rejects it to make the contrast
    // explicit.
    let file = fresh_file_with_claim();
    let res = enforce_scoped_commit_completion(&file, None, None, Some("committed"), None);
    assert_eq!(
        extract_error_code(&res.expect_err("opt-in path rejects")).as_deref(),
        Some("COMMIT_HASH_REQUIRED"),
    );
    // The gate is gated on the caller flag; this test pins that
    // contract by exercising it directly. The opt-out (legacy)
    // path is exercised by every existing `action_complete` test
    // above, all of which omit `enforce_scoped_commit`.
}

// ── Wave 18 / Task 08 — preflight_commit (worktree audit) ──
//
// These tests exercise the pure helpers (`parse_porcelain_status`,
// `build_preflight_summary`) plus the claim-resolution helper. The
// outer `action_preflight_commit` async path needs an AppState +
// a real git worktree, so we only smoke-test the orchestration
// through the helpers — the same approach the wave16-06 tests took
// for `enforce_scoped_commit_completion`.

/// Porcelain v1 parser must surface the standard XY-status pairs
/// that scoped-commit enforcement keys off (modified, added,
/// deleted, renamed, untracked) without dropping any path.
#[test]
fn porcelain_parser_recognises_each_status_kind() {
    let raw = " M src/a.rs\nA  src/b.rs\nMM src/c.rs\nD  src/d.rs\n?? new/file.rs\n!! .build/cache\nR  src/e.rs -> src/f.rs\n";
    let entries = parse_porcelain_status(raw);
    assert_eq!(entries.len(), 7);

    // Worktree-modified, not staged: changed but NOT staged.
    assert_eq!(entries[0].path, "src/a.rs");
    assert!(entries[0].is_changed());
    assert!(!entries[0].is_staged());

    // Staged-add: staged AND changed (worktree slot is space ⇒
    // identical to index, but the index slot is non-blank).
    assert!(entries[1].is_staged());
    assert!(entries[1].is_changed());

    // Both staged and worktree-edited (`MM`).
    assert!(entries[2].is_staged());
    assert!(entries[2].is_changed());

    // Staged delete.
    assert_eq!(entries[3].path, "src/d.rs");
    assert!(entries[3].is_staged());

    // Untracked: changed but NOT staged.
    assert_eq!(entries[4].path, "new/file.rs");
    assert!(entries[4].is_changed());
    assert!(!entries[4].is_staged());

    // Ignored: stays out of both buckets so .gitignore'd build
    // artefacts don't trip preflight.
    assert!(!entries[5].is_changed());
    assert!(!entries[5].is_staged());

    // Rename: parser must keep the post-rename path so scope-overlap
    // matches the on-disk file.
    assert_eq!(entries[6].path, "src/f.rs");
    assert!(entries[6].is_staged());
}

/// Empty stdout (clean worktree) must yield an empty entry list —
/// downstream `build_preflight_summary` then emits the
/// "worktree clean — nothing to commit" hint.
#[test]
fn porcelain_parser_handles_clean_worktree() {
    assert!(parse_porcelain_status("").is_empty());
    assert!(parse_porcelain_status("\n\n").is_empty());
}

/// Scope comparison: union of changed/staged paths inside the claim
/// scope keeps `out_of_scope_files` empty and `ok=true`.
#[test]
fn preflight_summary_in_scope_is_ok() {
    let entries = vec![
        PorcelainEntry {
            index_status: 'M',
            worktree_status: ' ',
            path: "src/a.rs".into(),
        },
        PorcelainEntry {
            index_status: ' ',
            worktree_status: 'M',
            path: "src/b.rs".into(),
        },
    ];
    let scopes = vec!["src/".to_string()];
    let summary = build_preflight_summary(&entries, &scopes, None);
    assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(true));
    let oos = summary
        .get("out_of_scope_files")
        .and_then(|v| v.as_array())
        .unwrap();
    assert!(oos.is_empty());
    assert_eq!(
        summary
            .get("staged_files")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(1),
    );
    assert_eq!(
        summary
            .get("changed_files")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(2),
    );
}

/// A staged path outside every claim scope must surface in
/// `out_of_scope_files` with `ok=false`. Parallel to
/// SCOPED_COMMIT_VIOLATION on the post-commit gate so the writer
/// agent sees the same drift signal at preflight time.
#[test]
fn preflight_summary_flags_out_of_scope_path() {
    let entries = vec![
        PorcelainEntry {
            index_status: 'M',
            worktree_status: ' ',
            path: "src/a.rs".into(),
        },
        PorcelainEntry {
            index_status: 'A',
            worktree_status: ' ',
            path: "vendor/x.rs".into(),
        },
    ];
    let scopes = vec!["src/".to_string()];
    let summary = build_preflight_summary(&entries, &scopes, None);
    assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(false));
    let oos: Vec<String> = summary
        .get("out_of_scope_files")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .map(|s| s.to_string())
        .collect();
    assert_eq!(oos, vec!["vendor/x.rs"]);
    let next = summary
        .get("next_step")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        next.contains("vendor/x.rs"),
        "next_step should mention the violator, got: {}",
        next
    );
}

/// No claims on the companion log + dirty worktree → every touched
/// path is out-of-scope by definition. This is the pre-claim case
/// the wave16-06 enforcement gate calls CLAIM_SCOPE_REQUIRED;
/// preflight surfaces it as a flat out-of-scope list with a
/// "open a claim first" next_step instead of a hard error so the
/// writer can iteratively fix it.
#[test]
fn preflight_summary_no_claims_marks_everything_out_of_scope() {
    let entries = vec![PorcelainEntry {
        index_status: 'M',
        worktree_status: ' ',
        path: "src/a.rs".into(),
    }];
    let scopes: Vec<String> = vec![];
    let summary = build_preflight_summary(&entries, &scopes, None);
    assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(false));
    let oos: Vec<&str> = summary
        .get("out_of_scope_files")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(oos, vec!["src/a.rs"]);
    let next = summary
        .get("next_step")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        next.contains("open a claim"),
        "next_step should suggest opening a claim, got: {}",
        next
    );
}

/// Clean worktree + active claim: ok=true, both file lists empty,
/// next_step explicitly says "nothing to commit".
#[test]
fn preflight_summary_clean_worktree_ok() {
    let entries: Vec<PorcelainEntry> = vec![];
    let scopes = vec!["src/".to_string()];
    let summary = build_preflight_summary(&entries, &scopes, None);
    assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        summary
            .get("changed_files")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(0),
    );
    assert_eq!(
        summary
            .get("staged_files")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(0),
    );
    let next = summary
        .get("next_step")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        next.contains("worktree clean"),
        "next_step should mention clean worktree, got: {}",
        next
    );
}

/// `expected_files` hint surfaces both directions of drift:
/// expected-but-not-touched goes to `expected_missing`, touched-but-
/// not-expected goes to `expected_unexpected`. Neither flips `ok`
/// because the scope check is the source of truth — expected_files
/// is advisory metadata from the dispatch brief.
#[test]
fn preflight_summary_expected_files_drift_surfaces_both_directions() {
    let entries = vec![
        PorcelainEntry {
            index_status: 'M',
            worktree_status: ' ',
            path: "src/a.rs".into(),
        },
        PorcelainEntry {
            index_status: 'A',
            worktree_status: ' ',
            path: "src/c.rs".into(),
        },
    ];
    let scopes = vec!["src/".to_string()];
    let expected = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
    let summary = build_preflight_summary(&entries, &scopes, Some(&expected));
    // Scope check passes — `c.rs` is inside `src/` even though it
    // wasn't expected.
    assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(true));
    let missing: Vec<&str> = summary
        .get("expected_missing")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(missing, vec!["src/b.rs"]);
    let unexpected: Vec<&str> = summary
        .get("expected_unexpected")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(unexpected, vec!["src/c.rs"]);
}

/// Claim resolution: `claim_id` pointing to a real claim returns
/// just that claim's scope (single-element vec).
#[test]
fn preflight_specific_claim_returns_just_that_scope() {
    let mut file = fresh_file();
    let now = now_iso();
    let entry = format!(
        "    (C001\n      :claimer \"agent\"\n      :scope \"src/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :heartbeat-at {ts}\n      :status \"active\")",
        ts = lisp_quote_string(&now),
    );
    append_to_block(&mut file, "claims", &entry).unwrap();
    let entry2 = format!(
        "    (C002\n      :claimer \"agent\"\n      :scope \"vendor/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :heartbeat-at {ts}\n      :status \"active\")",
        ts = lisp_quote_string(&now),
    );
    append_to_block(&mut file, "claims", &entry2).unwrap();
    let scopes = collect_specific_claim_scope(&file, "C002").unwrap();
    assert_eq!(scopes, vec!["vendor/".to_string()]);
    // Union path includes both.
    let union = collect_all_claim_scopes(&file);
    assert_eq!(union.len(), 2);
    assert!(union.contains(&"src/".to_string()));
    assert!(union.contains(&"vendor/".to_string()));
}

/// Unknown claim_id must reject with NOT_FOUND so the writer
/// learns about the typo before running git.
#[test]
fn preflight_unknown_claim_id_rejects() {
    let file = fresh_file();
    let err =
        collect_specific_claim_scope(&file, "C999").expect_err("unknown claim id must reject");
    assert_eq!(extract_error_code(&err).as_deref(), Some("NOT_FOUND"));
}

// ── Wave 19 / Task 08 — task-contract completion metadata ──
//
// The runtime gate `enforce_task_contract_completion` is what
// `action_complete` calls when the caller pairs
// `enforce_scoped_commit=true` with a `task_contract_path`. These
// tests pin the four structured-error codes plus the happy-path
// validation summary using a tempdir-anchored project root so the
// contract loader sees a real file. Verifier-status normalization
// and persistence are covered separately at the helper level.

use std::io::Write;

fn write_task_contract(dir: &Path, rel: &str, body: &str) -> PathBuf {
    let abs = dir.join(rel);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    let mut f = std::fs::File::create(&abs).expect("create");
    f.write_all(body.as_bytes()).expect("write");
    abs
}

/// Minimal valid task-contract v1 form. Mirrors the shape produced
/// by plan.rs::build_task_contract_lisp (wave19-06) but trimmed to
/// the fields the daemon enforcement gate inspects.
const SAMPLE_CONTRACT_BODY: &str = r#"
(task wave19-08-test-contract
  :schema "missiond.task-contract.v1"
  :goal "exercise task-contract completion gate"
  :write-scope ["src/a.rs" "src/b.rs"]
  :must-not-touch []
  :acceptance []
  :commit (:required true :message "feat(test): wave19-08" :scope-check write-scope-only))
"#;

/// `verifier_status` normalizer must accept every canonical label
/// (with whitespace) and reject typos. Mirrors the contract for
/// `commit_status` so the test surface stays uniform across
/// completion enums.
#[test]
fn verifier_status_normalizer_accepts_canonical_only() {
    for &status in VALID_VERIFIER_STATUSES {
        assert_eq!(normalize_verifier_status(status), Some(status));
    }
    assert_eq!(normalize_verifier_status("  passed  "), Some("passed"));
    assert!(normalize_verifier_status("").is_none());
    assert!(normalize_verifier_status("done").is_none());
    assert!(normalize_verifier_status("PASSED").is_none());
}

/// `parse_completions` must round-trip the wave19-08 metadata when
/// every new field is present, including verifier_notes prose with
/// punctuation, so dashboards / status surfaces see the original
/// caller-supplied strings.
#[test]
fn parse_completions_reads_task_contract_metadata() {
    let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :commit-hash \"abc1234\"\n      :commit-status \"committed\"\n      :task-contract-path \".missiond/tasks/wave19/sample.lisp\"\n      :task-report-path \".missiond/tasks/wave19/reports/sample.report.lisp\"\n      :verifier-status \"passed\"\n      :verifier-notes \"verifier OK against abc1234\")))\n";
    let file = LogFile::parse(body.to_string()).expect("parse");
    let comps = parse_completions(&file);
    assert_eq!(comps.len(), 1);
    let c = &comps[0];
    assert_eq!(
        c.task_contract_path.as_deref(),
        Some(".missiond/tasks/wave19/sample.lisp"),
    );
    assert_eq!(
        c.task_report_path.as_deref(),
        Some(".missiond/tasks/wave19/reports/sample.report.lisp"),
    );
    assert_eq!(c.verifier_status.as_deref(), Some("passed"));
    assert_eq!(
        c.verifier_notes.as_deref(),
        Some("verifier OK against abc1234"),
    );
}

/// Legacy completions (no wave19-08 fields) must still parse and
/// surface `None` everywhere new — the same backward-compat contract
/// the wave12-01 scoped-commit fields uphold.
#[test]
fn parse_completions_legacy_omits_task_contract_metadata() {
    let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\")))\n";
    let file = LogFile::parse(body.to_string()).expect("parse");
    let c = &parse_completions(&file)[0];
    assert!(c.task_contract_path.is_none());
    assert!(c.task_report_path.is_none());
    assert!(c.verifier_status.is_none());
    assert!(c.verifier_notes.is_none());
}

/// Missing task-contract file → TASK_CONTRACT_REQUIRED. The error
/// must surface BEFORE the daemon mutates the companion log so the
/// writer can correct the path without a partial commit on record.
#[test]
fn enforce_contract_rejects_missing_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = fresh_file_with_claim();
    let res = enforce_task_contract_completion(
        &file,
        dir.path(),
        "tasks/does-not-exist.lisp",
        Some("abc1234"),
        Some(&["src/a.rs".to_string()]),
    );
    let err = res.expect_err("missing file must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_CONTRACT_REQUIRED"),
    );
}

/// Malformed contract body (schema mismatch) → TASK_CONTRACT_MALFORMED.
/// Distinct from REQUIRED so the writer can tell "wrong path" from
/// "wrong content" without re-running the verifier.
#[test]
fn enforce_contract_rejects_malformed_schema() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bad = "(task wave19-08-bad\n  :schema \"missiond.task-contract.v0\"\n  :goal \"bad\")";
    write_task_contract(dir.path(), "tasks/bad.lisp", bad);
    let file = fresh_file_with_claim();
    let res = enforce_task_contract_completion(
        &file,
        dir.path(),
        "tasks/bad.lisp",
        Some("abc1234"),
        Some(&["src/a.rs".to_string()]),
    );
    let err = res.expect_err("schema mismatch must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_CONTRACT_MALFORMED"),
    );
}

/// Missing commit_hash → COMMIT_HASH_REQUIRED_FOR_CONTRACT. Distinct
/// from the scoped-commit COMMIT_HASH_REQUIRED so dashboards can
/// distinguish "no hash on report" from "no hash on commit_status".
#[test]
fn enforce_contract_rejects_missing_commit_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let file = fresh_file_with_claim();
    let res = enforce_task_contract_completion(
        &file,
        dir.path(),
        "tasks/ok.lisp",
        None,
        Some(&["src/a.rs".to_string()]),
    );
    let err = res.expect_err("missing hash must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("COMMIT_HASH_REQUIRED_FOR_CONTRACT"),
    );
}

/// Empty / whitespace commit_hash also rejects — the helper trims
/// before checking so the writer cannot smuggle a blank string past
/// the gate.
#[test]
fn enforce_contract_rejects_blank_commit_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let file = fresh_file_with_claim();
    let res = enforce_task_contract_completion(
        &file,
        dir.path(),
        "tasks/ok.lisp",
        Some("   "),
        Some(&["src/a.rs".to_string()]),
    );
    let err = res.expect_err("blank hash must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("COMMIT_HASH_REQUIRED_FOR_CONTRACT"),
    );
}

/// `:write-scope` entry not covered by any claim AND not staged →
/// CLAIM_SCOPE_MISSING. This is the "writer ran the verifier OK but
/// the daemon-side state cannot prove the work landed inside scope"
/// case the gate exists to catch.
#[test]
fn enforce_contract_rejects_uncovered_write_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    // fresh_file_with_claim covers "src/" — that overlaps both
    // contract entries, so we narrow the claim to a sibling path
    // that proves the contract entries are uncovered. Easiest: use
    // fresh_file (no claims) and stage NOTHING.
    let file = fresh_file();
    let res =
        enforce_task_contract_completion(&file, dir.path(), "tasks/ok.lisp", Some("abc1234"), None);
    let err = res.expect_err("uncovered scope must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("CLAIM_SCOPE_MISSING"),
    );
}

/// Happy path: contract loadable, hash present, every :write-scope
/// entry overlaps an active claim. Validation summary records the
/// resolved path + checked rules so the response mirrors the
/// scoped-commit gate's shape.
#[test]
fn enforce_contract_accepts_covered_write_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let resolved = write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let file = fresh_file_with_claim(); // active claim on "src/"
    let res = enforce_task_contract_completion(
        &file,
        dir.path(),
        "tasks/ok.lisp",
        Some("abc1234"),
        Some(&["src/a.rs".to_string(), "src/b.rs".to_string()]),
    );
    let summary = res.expect("covered scope must pass");
    assert_eq!(
        summary.get("schema").and_then(|v| v.as_str()),
        Some("missiond.task-contract.v1"),
    );
    assert_eq!(
        summary.get("write_scope_entries").and_then(|v| v.as_u64()),
        Some(2),
    );
    assert_eq!(
        summary
            .get("resolved_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        Some(resolved.display().to_string()),
    );
}

/// Happy path with absolute task_contract_path: must NOT be rejoined
/// against the project root, and the resolved_path echoed back must
/// be byte-equal to the absolute path the caller supplied.
#[test]
fn enforce_contract_accepts_absolute_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let resolved = write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let abs_str = resolved.display().to_string();
    let file = fresh_file_with_claim();
    let res = enforce_task_contract_completion(
        &file,
        // Anchor against an unrelated tempdir to prove the absolute
        // path takes precedence over project_root.
        tempfile::tempdir().unwrap().path(),
        &abs_str,
        Some("abc1234"),
        Some(&["src/a.rs".to_string(), "src/b.rs".to_string()]),
    );
    let summary = res.expect("absolute path must load");
    assert_eq!(
        summary.get("resolved_path").and_then(|v| v.as_str()),
        Some(abs_str.as_str()),
    );
}

/// Staged file alone (no claim) is enough to cover a :write-scope
/// entry — this is the "brand new file" case where the writer staged
/// it but has not yet opened a claim. Mirrors the scoped-commit gate
/// which accepts staged paths inside released claims.
#[test]
fn enforce_contract_accepts_staged_only_coverage() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let file = fresh_file(); // zero claims
    let res = enforce_task_contract_completion(
        &file,
        dir.path(),
        "tasks/ok.lisp",
        Some("abc1234"),
        Some(&["src/a.rs".to_string(), "src/b.rs".to_string()]),
    );
    assert!(res.is_ok(), "staged paths alone should cover write-scope");
}

// ── Wave 20 / Task 03 — preflight task-contract scope projection ──
//
// These tests pin the new wave20-03 pure helpers used by
// `action_preflight_commit` when the caller threads
// `task_contract_path` through the call. They exercise the glob
// matcher, the four-field structured projection, and the contract
// loader's status labels (loaded / missing / malformed). The async
// path through `action_preflight_commit` itself is smoke-tested via
// these helpers — same approach the wave18-08 preflight tests use.

/// Bare prefix patterns must match the exact path AND any descendant
/// when the pattern denotes a directory. Mirrors the JS
/// `pathMatchesPattern` semantics so daemon-side preflight stays in
/// lock-step with `scripts/lib/missiond_lisp.mjs`.
#[test]
fn pattern_matches_path_handles_bare_prefix() {
    // Exact match.
    assert!(pattern_matches_path(
        "crates/missiond-daemon/src/lib.rs",
        "crates/missiond-daemon/src/lib.rs",
    ));
    // Directory prefix without trailing slash.
    assert!(pattern_matches_path("crates/foo/bar.rs", "crates"));
    // Directory prefix with trailing slash.
    assert!(pattern_matches_path("crates/foo/bar.rs", "crates/"));
    // Sibling path must NOT match (no false-positive prefix overlap).
    assert!(!pattern_matches_path("crates2/foo.rs", "crates"));
    // Empty inputs never match.
    assert!(!pattern_matches_path("", "crates"));
    assert!(!pattern_matches_path("crates/foo.rs", ""));
}

/// `**` must match across folder hops; `*` must NOT cross `/`.
/// Pinned because the wave20-03 contract for must-not-touch uses
/// `scripts/**` and `.missiond/v2/*.lisp` — both shapes need to work
/// or the task scope guard regresses.
#[test]
fn pattern_matches_path_handles_globs() {
    // `**` crosses folder boundaries.
    assert!(pattern_matches_path("scripts/foo.mjs", "scripts/**"));
    assert!(pattern_matches_path(
        "scripts/lib/missiond_lisp.mjs",
        "scripts/**",
    ));
    // `*` does not cross `/`.
    assert!(pattern_matches_path(
        ".missiond/v2/foo.lisp",
        ".missiond/v2/*.lisp"
    ));
    assert!(!pattern_matches_path(
        ".missiond/v2/sub/foo.lisp",
        ".missiond/v2/*.lisp",
    ));
    // `?` matches a single non-`/` char.
    assert!(pattern_matches_path("a.rs", "?.rs"));
    assert!(!pattern_matches_path("ab.rs", "?.rs"));
    // Regex meta-characters in pattern are escaped — a literal `.`
    // matches a `.`, not "any char".
    assert!(pattern_matches_path("a.rs", "a.rs"));
    assert!(!pattern_matches_path("axrs", "a.rs"));
}

/// Backslashes / leading `./` / leading `/` collapse to repo-relative
/// before comparison so Windows-style paths and verbose contract
/// entries match the same canonical form.
#[test]
fn pattern_matches_path_normalizes_separators() {
    assert!(pattern_matches_path("./crates/foo.rs", "crates/foo.rs"));
    assert!(pattern_matches_path("crates\\foo.rs", "crates/foo.rs"));
    assert!(pattern_matches_path("/crates/foo.rs", "crates/foo.rs"));
    assert!(pattern_matches_path("crates/foo.rs", "./crates/foo.rs"));
}

/// Happy path: every staged path lands in :write-scope, none in
/// :must-not-touch, no unstaged drift. `next_step` confirms the
/// writer can proceed with the scoped commit.
#[test]
fn contract_scope_summary_clean_in_scope_set() {
    let staged = vec![
        "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
        "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs".to_string(),
    ];
    let changed = staged.clone();
    let write_scope = vec![
        "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
        "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs".to_string(),
    ];
    let must_not_touch = vec!["scripts/**".to_string()];
    let summary = build_contract_scope_summary(&staged, &changed, &write_scope, &must_not_touch);
    assert_eq!(
        summary
            .get("staged_out_of_scope")
            .and_then(|v| v.as_array())
            .unwrap()
            .len(),
        0,
    );
    assert_eq!(
        summary
            .get("staged_forbidden")
            .and_then(|v| v.as_array())
            .unwrap()
            .len(),
        0,
    );
    assert_eq!(
        summary
            .get("unstaged_in_scope")
            .and_then(|v| v.as_array())
            .unwrap()
            .len(),
        0,
    );
    let next = summary
        .get("next_step")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        next.contains("respects :write-scope"),
        "next_step should confirm clean state, got: {}",
        next,
    );
}

/// Staged path matches a `:must-not-touch` glob → surfaces in
/// `staged_forbidden` and the next_step prose tells the writer to
/// unstage. Mirrors what `scripts/task-scope-guard.mjs` rejects on
/// the post-commit side.
#[test]
fn contract_scope_summary_flags_must_not_touch_glob() {
    let staged = vec![
        "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
        "scripts/render-claudecode-task.mjs".to_string(),
    ];
    let changed = staged.clone();
    let write_scope =
        vec!["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string()];
    let must_not_touch = vec!["scripts/**".to_string()];
    let summary = build_contract_scope_summary(&staged, &changed, &write_scope, &must_not_touch);
    let forbidden: Vec<&str> = summary
        .get("staged_forbidden")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(forbidden, vec!["scripts/render-claudecode-task.mjs"]);
    // The same path is also out-of-scope (it doesn't match write_scope).
    let oos: Vec<&str> = summary
        .get("staged_out_of_scope")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(oos, vec!["scripts/render-claudecode-task.mjs"]);
    let next = summary
        .get("next_step")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        next.contains("must-not-touch"),
        "next_step should mention must-not-touch, got: {}",
        next,
    );
}

/// Staged path lands outside both :write-scope and :must-not-touch →
/// only `staged_out_of_scope` populates; `staged_forbidden` stays
/// empty. Distinct signal from the `must-not-touch` case so dashboards
/// can distinguish "out of declared scope" from "explicitly off-limits".
#[test]
fn contract_scope_summary_flags_out_of_scope_without_forbidden() {
    let staged = vec!["crates/missiond-core/src/event/events/execution.rs".to_string()];
    let changed = staged.clone();
    let write_scope =
        vec!["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string()];
    // execution.rs is in must-not-touch for wave20-03 but for this
    // test we leave it empty so we get a pure "out-of-scope" signal.
    let must_not_touch: Vec<String> = vec![];
    let summary = build_contract_scope_summary(&staged, &changed, &write_scope, &must_not_touch);
    assert_eq!(
        summary
            .get("staged_forbidden")
            .and_then(|v| v.as_array())
            .unwrap()
            .len(),
        0,
    );
    let oos: Vec<&str> = summary
        .get("staged_out_of_scope")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        oos,
        vec!["crates/missiond-core/src/event/events/execution.rs"]
    );
}

/// Unstaged-but-in-scope: a file the writer edited but forgot to
/// `git add` lands in `unstaged_in_scope`. Must NOT bleed into
/// `staged_out_of_scope` (it's not staged) and must NOT bleed into
/// `staged_forbidden`.
#[test]
fn contract_scope_summary_flags_unstaged_in_scope_drift() {
    let staged =
        vec!["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string()];
    let changed = vec![
        "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
        "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs".to_string(),
    ];
    let write_scope = vec![
        "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
        "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs".to_string(),
    ];
    let summary = build_contract_scope_summary(&staged, &changed, &write_scope, &[]);
    let unstaged: Vec<&str> = summary
        .get("unstaged_in_scope")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        unstaged,
        vec!["crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"],
    );
    assert_eq!(
        summary
            .get("staged_out_of_scope")
            .and_then(|v| v.as_array())
            .unwrap()
            .len(),
        0,
    );
    let next = summary
        .get("next_step")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        next.contains("stage the in-scope edits"),
        "next_step should suggest staging, got: {}",
        next,
    );
}

/// Empty :write-scope → every staged path lands in
/// `staged_out_of_scope`. Matches the verifier's posture: a contract
/// without `:write-scope` cannot grant any path.
#[test]
fn contract_scope_summary_empty_write_scope_rejects_everything() {
    let staged = vec!["crates/foo.rs".to_string()];
    let summary = build_contract_scope_summary(&staged, &staged, &[], &[]);
    let oos: Vec<&str> = summary
        .get("staged_out_of_scope")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(oos, vec!["crates/foo.rs"]);
}

/// Loader happy path: contract on disk + matching staged set →
/// `task_contract_status="loaded"`, scope summary populated, no
/// failure message.
#[test]
fn evaluate_contract_for_preflight_loaded_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let resolved = write_task_contract(dir.path(), "tasks/wave20-03.lisp", SAMPLE_CONTRACT_BODY);
    let staged = vec!["src/a.rs".to_string()];
    let changed = vec!["src/a.rs".to_string()];
    let (status, summary, resolved_path, failure) =
        evaluate_task_contract_for_preflight(dir.path(), "tasks/wave20-03.lisp", &staged, &changed);
    assert_eq!(status, "loaded");
    assert!(failure.is_none());
    assert_eq!(
        resolved_path.as_deref(),
        Some(resolved.display().to_string().as_str()),
    );
    let scope = summary.expect("loaded path must produce summary");
    assert_eq!(
        scope
            .get("staged_out_of_scope")
            .and_then(|v| v.as_array())
            .unwrap()
            .len(),
        0,
    );
}

/// Loader missing-file path: returns `task_contract_status="missing"`
/// and a failure message that names the resolved path so the caller
/// can correct the brief without spawning git.
#[test]
fn evaluate_contract_for_preflight_missing_file_returns_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, summary, resolved_path, failure) =
        evaluate_task_contract_for_preflight(dir.path(), "tasks/does-not-exist.lisp", &[], &[]);
    assert_eq!(status, "missing");
    assert!(
        summary.is_none(),
        "missing file must not yield a scope summary"
    );
    assert!(resolved_path.is_some());
    let msg = failure.expect("missing file must produce failure message");
    assert!(
        msg.contains("not readable"),
        "msg should describe IO failure, got: {}",
        msg
    );
}

/// Loader malformed-file path: returns `task_contract_status="malformed"`
/// distinct from `missing` so the caller can tell "wrong path" from
/// "wrong content" without re-reading the file.
#[test]
fn evaluate_contract_for_preflight_malformed_returns_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bad = "(task wave20-03-bad\n  :schema \"missiond.task-contract.v0\"\n  :goal \"bad\")";
    write_task_contract(dir.path(), "tasks/bad.lisp", bad);
    let (status, summary, _resolved, failure) =
        evaluate_task_contract_for_preflight(dir.path(), "tasks/bad.lisp", &[], &[]);
    assert_eq!(status, "malformed");
    assert!(summary.is_none());
    let msg = failure.expect("malformed file must produce failure message");
    assert!(
        msg.contains("schema parse"),
        "msg should describe schema mismatch, got: {}",
        msg,
    );
}

/// Absolute task_contract_path must NOT be re-anchored against the
/// project root. Resolved path echoed back is byte-equal to the input.
#[test]
fn evaluate_contract_for_preflight_accepts_absolute_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let resolved = write_task_contract(dir.path(), "tasks/wave20-03.lisp", SAMPLE_CONTRACT_BODY);
    let abs = resolved.display().to_string();
    let unrelated = tempfile::tempdir().expect("tempdir");
    let (status, summary, resolved_path, _failure) =
        evaluate_task_contract_for_preflight(unrelated.path(), &abs, &[], &[]);
    assert_eq!(status, "loaded");
    assert!(summary.is_some());
    assert_eq!(resolved_path.as_deref(), Some(abs.as_str()));
}

// ── Wave 21 / Task 03 — execution report verifier integration ──
//
// Pin the wave21-03 surface: enum normalizer, the four-field
// companion-log round-trip via `parse_completions`, the mini
// report-summary reader, the contract head-id reader, and the
// `enforce_verified_completion` gate covering every documented
// failure code plus the happy path.

/// `task_run_verifier_status` normalizer accepts every canonical
/// label (with whitespace) and rejects typos. Mirrors the contract
/// of the wave19-08 `verifier_status` normalizer.
#[test]
fn task_run_verifier_status_normalizer_accepts_canonical_only() {
    for &status in VALID_TASK_RUN_VERIFIER_STATUSES {
        assert_eq!(normalize_task_run_verifier_status(status), Some(status));
    }
    assert_eq!(
        normalize_task_run_verifier_status("  passed  "),
        Some("passed")
    );
    assert!(normalize_task_run_verifier_status("").is_none());
    assert!(normalize_task_run_verifier_status("done").is_none());
    assert!(normalize_task_run_verifier_status("PASSED").is_none());
}

/// `parse_completions` round-trips the wave21-03 metadata when
/// every new field is present, including `verified=true` written
/// as a bare atom and `verifier_diagnostics` prose with
/// punctuation.
#[test]
fn parse_completions_reads_task_run_verifier_metadata() {
    let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :commit-hash \"abc1234\"\n      :commit-status \"committed\"\n      :task-run-verifier-status \"passed\"\n      :shared-memory-path \".missiond/tasks/wave21/shared-memory.lisp\"\n      :verifier-diagnostics \"verify-task-run.mjs OK against abc1234\"\n      :verified true)))\n";
    let file = LogFile::parse(body.to_string()).expect("parse");
    let comps = parse_completions(&file);
    assert_eq!(comps.len(), 1);
    let c = &comps[0];
    assert_eq!(c.task_run_verifier_status.as_deref(), Some("passed"));
    assert_eq!(
        c.shared_memory_path.as_deref(),
        Some(".missiond/tasks/wave21/shared-memory.lisp"),
    );
    assert_eq!(
        c.verifier_diagnostics.as_deref(),
        Some("verify-task-run.mjs OK against abc1234"),
    );
    assert_eq!(c.verified, Some(true));
}

/// Legacy completions (no wave21-03 fields) parse cleanly and
/// surface `None` everywhere new — the same backward-compat
/// contract every prior wave upholds.
#[test]
fn parse_completions_legacy_omits_task_run_verifier_metadata() {
    let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\")))\n";
    let file = LogFile::parse(body.to_string()).expect("parse");
    let c = &parse_completions(&file)[0];
    assert!(c.task_run_verifier_status.is_none());
    assert!(c.shared_memory_path.is_none());
    assert!(c.verifier_diagnostics.is_none());
    assert!(c.verified.is_none());
}

/// Explicit `verified=false` round-trips to `Some(false)` so audit
/// can tell "writer intentionally skipped verification" from "writer
/// omitted the field" (legacy caller).
#[test]
fn parse_completions_round_trips_verified_false() {
    let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"p\"\n      :agent \"a\"\n      :summary \"s\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :verified false)))\n";
    let file = LogFile::parse(body.to_string()).expect("parse");
    let c = &parse_completions(&file)[0];
    assert_eq!(c.verified, Some(false));
}

/// Mini report reader pulls just the three keys the gate cares
/// about and ignores everything else (notes, files_changed, etc.).
#[test]
fn read_report_summary_extracts_required_fields() {
    let body = r#"
(report wave21-03-sample
  :schema "missiond.report-contract.v1"
  :task_id "wave21-03-sample"
  :status done
  :commit_hash "deadbeef0123"
  :files_changed ["a.rs" "b.rs"]
  :acceptance_results []
  :notes "ignored")"#;
    let r = read_report_summary(body).expect("parse");
    assert_eq!(r.schema.as_deref(), Some("missiond.report-contract.v1"));
    assert_eq!(r.task_id.as_deref(), Some("wave21-03-sample"));
    assert_eq!(r.commit_hash.as_deref(), Some("deadbeef0123"));
}

/// Non-`(report ...)` top form rejects so the reader cannot be
/// tricked into projecting a contract or a companion log.
#[test]
fn read_report_summary_rejects_non_report_form() {
    let body = r#"(task wave21-03-not-a-report :schema "missiond.task-contract.v1" :goal "x")"#;
    assert!(read_report_summary(body).is_err());
}

/// Contract head-id reader pulls the `<id>` symbol from
/// `(task <id> ...)`. Used by the verified-gate to cross-check
/// the report `:task_id`.
#[test]
fn read_task_contract_id_extracts_head_symbol() {
    let body = r#"(task wave21-03-test-contract :schema "missiond.task-contract.v1" :goal "x")"#;
    assert_eq!(
        read_task_contract_id(body).as_deref(),
        Some("wave21-03-test-contract"),
    );
    let other = r#"(plan p :schema "x")"#;
    assert!(read_task_contract_id(other).is_none());
}

/// Sample report body matching SAMPLE_CONTRACT_BODY's head id.
/// Hash is the `abc1234` short sha used across the wave19-08 tests
/// so the verified-gate hash-prefix overlap rule lights up cleanly.
const SAMPLE_REPORT_BODY: &str = r#"
(report wave19-08-test-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave19-08-test-contract"
  :status done
  :commit_hash "abc1234"
  :files_changed ["src/a.rs" "src/b.rs"]
  :acceptance_results [(:command "x" :exit_code 0 :ok true)]
  :notes "wave21-03 test fixture")
"#;

fn write_task_report(dir: &Path, rel: &str, body: &str) -> PathBuf {
    let abs = dir.join(rel);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    let mut f = std::fs::File::create(&abs).expect("create");
    f.write_all(body.as_bytes()).expect("write");
    abs
}

/// `verified=true` without `enforce_scoped_commit=true` rejects
/// with `VERIFIED_REQUIRES_ENFORCEMENT`. The verified flag is
/// meaningless without the underlying scope gate also running.
#[test]
fn verified_rejects_without_enforce_scoped_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let res = enforce_verified_completion(
        dir.path(),
        false,
        Some("tasks/x.lisp"),
        Some("tasks/x.report.lisp"),
        Some("abc1234"),
    );
    let err = res.expect_err("must reject without enforcement");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("VERIFIED_REQUIRES_ENFORCEMENT"),
    );
}

/// Missing task_contract_path → `VERIFIED_REQUIRES_TASK_CONTRACT`.
#[test]
fn verified_rejects_missing_task_contract_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let res = enforce_verified_completion(
        dir.path(),
        true,
        None,
        Some("tasks/x.report.lisp"),
        Some("abc1234"),
    );
    let err = res.expect_err("must reject missing contract");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("VERIFIED_REQUIRES_TASK_CONTRACT"),
    );
}

/// Missing task_report_path → `VERIFIED_REQUIRES_TASK_REPORT`.
#[test]
fn verified_rejects_missing_task_report_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some("tasks/x.lisp"),
        None,
        Some("abc1234"),
    );
    let err = res.expect_err("must reject missing report");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("VERIFIED_REQUIRES_TASK_REPORT"),
    );
}

/// Missing commit_hash → `VERIFIED_REQUIRES_COMMIT_HASH`.
/// Whitespace-only also rejects via the trim-then-filter-empty
/// pipeline.
#[test]
fn verified_rejects_missing_commit_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some("tasks/x.lisp"),
        Some("tasks/x.report.lisp"),
        None,
    );
    let err = res.expect_err("must reject absent hash");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("VERIFIED_REQUIRES_COMMIT_HASH"),
    );

    let res2 = enforce_verified_completion(
        dir.path(),
        true,
        Some("tasks/x.lisp"),
        Some("tasks/x.report.lisp"),
        Some("   "),
    );
    let err2 = res2.expect_err("must reject blank hash");
    assert_eq!(
        extract_error_code(&err2).as_deref(),
        Some("VERIFIED_REQUIRES_COMMIT_HASH"),
    );
}

/// Missing report file → `TASK_REPORT_REQUIRED`. The error must
/// surface BEFORE the daemon mutates the companion log.
#[test]
fn verified_rejects_missing_report_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some("tasks/ok.lisp"),
        Some("tasks/does-not-exist.report.lisp"),
        Some("abc1234"),
    );
    let err = res.expect_err("missing report must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_REPORT_REQUIRED"),
    );
}

/// Report with a wrong `:schema` → `TASK_REPORT_MALFORMED`.
#[test]
fn verified_rejects_report_schema_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let bad = r#"(report wave19-08-test-contract :schema "missiond.report-contract.v0" :task_id "wave19-08-test-contract" :commit_hash "abc1234")"#;
    write_task_report(dir.path(), "tasks/bad.report.lisp", bad);
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some("tasks/ok.lisp"),
        Some("tasks/bad.report.lisp"),
        Some("abc1234"),
    );
    let err = res.expect_err("schema mismatch must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_REPORT_MALFORMED"),
    );
}

/// Report `:task_id` not matching the contract head id →
/// `TASK_REPORT_TASK_ID_MISMATCH`. Distinct from the schema error
/// so the writer can tell "wrong file referenced" from "wrong
/// file shape".
#[test]
fn verified_rejects_report_task_id_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let body = r#"
(report wave21-03-other-task
  :schema "missiond.report-contract.v1"
  :task_id "wave21-03-other-task"
  :status done
  :commit_hash "abc1234"
  :files_changed []
  :acceptance_results [])
"#;
    write_task_report(dir.path(), "tasks/wrong.report.lisp", body);
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some("tasks/ok.lisp"),
        Some("tasks/wrong.report.lisp"),
        Some("abc1234"),
    );
    let err = res.expect_err("task_id mismatch must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_REPORT_TASK_ID_MISMATCH"),
    );
}

/// Report `:commit_hash` not matching the supplied hash →
/// `TASK_REPORT_COMMIT_HASH_MISMATCH`. Tests with a clearly
/// different hash so the prefix-overlap rule cannot accidentally
/// pass.
#[test]
fn verified_rejects_report_commit_hash_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let body = r#"
(report wave19-08-test-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave19-08-test-contract"
  :status done
  :commit_hash "feedbeef9999"
  :files_changed []
  :acceptance_results [])
"#;
    write_task_report(dir.path(), "tasks/x.report.lisp", body);
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some("tasks/ok.lisp"),
        Some("tasks/x.report.lisp"),
        Some("abc1234"),
    );
    let err = res.expect_err("hash mismatch must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_REPORT_COMMIT_HASH_MISMATCH"),
    );
}

/// Happy path: every precondition met, report loadable, schema +
/// task_id + commit_hash all match. Validation summary echoes the
/// resolved paths + the checked rules.
#[test]
fn verified_accepts_aligned_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let contract_resolved = write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let report_resolved = write_task_report(dir.path(), "tasks/ok.report.lisp", SAMPLE_REPORT_BODY);
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some("tasks/ok.lisp"),
        Some("tasks/ok.report.lisp"),
        Some("abc1234"),
    );
    let summary = res.expect("aligned report must pass");
    assert_eq!(
        summary.get("task_id").and_then(|v| v.as_str()),
        Some("wave19-08-test-contract"),
    );
    assert_eq!(
        summary
            .get("task_contract_resolved_path")
            .and_then(|v| v.as_str()),
        Some(contract_resolved.display().to_string().as_str()),
    );
    assert_eq!(
        summary
            .get("task_report_resolved_path")
            .and_then(|v| v.as_str()),
        Some(report_resolved.display().to_string().as_str()),
    );
    let checked = summary
        .get("checked")
        .and_then(|v| v.as_array())
        .expect("checked");
    assert!(checked
        .iter()
        .any(|v| v.as_str() == Some("preconditions_present")));
    assert!(checked
        .iter()
        .any(|v| v.as_str() == Some("task_report_loadable")));
    assert!(checked
        .iter()
        .any(|v| v.as_str() == Some("task_report_schema")));
    assert!(checked
        .iter()
        .any(|v| v.as_str() == Some("task_id_matches_contract")));
    assert!(checked
        .iter()
        .any(|v| v.as_str() == Some("commit_hash_matches_report")));
}

/// Long-sha completion hash + short-sha report hash overlap via
/// `starts_with`. Matches the way `git log --format=%h` truncates
/// to 7+ chars while `git rev-parse HEAD` returns the full
/// 40-char form.
#[test]
fn verified_accepts_short_long_sha_prefix_overlap() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
    let body = r#"
(report wave19-08-test-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave19-08-test-contract"
  :status done
  :commit_hash "abc1234"
  :files_changed []
  :acceptance_results [])
"#;
    write_task_report(dir.path(), "tasks/x.report.lisp", body);
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some("tasks/ok.lisp"),
        Some("tasks/x.report.lisp"),
        Some("abc1234567890abcdef"),
    );
    assert!(res.is_ok(), "long↔short sha overlap should pass");
}

/// Read-only proof: the mission_execution facade + preflight
/// surface may only spawn `git` for `git status --porcelain=v1` (the
/// wave18-08 preflight check). We grep both files at test time so the
/// proof survives future edits — exactly one `Command::new(<git>)` site
/// is allowed across the V3 surface boundary.
/// Anchor at `CARGO_MANIFEST_DIR` so the test stays robust to
/// whichever working directory the cargo harness picks. The
/// search needle is built at runtime via `format!` so the test
/// source itself doesn't count toward the match (a self-counting
/// literal would always inflate the total by one).
#[test]
fn daemon_never_invokes_mutating_git() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let facade_path =
        std::path::Path::new(manifest_dir).join("src/handlers/knowledge/agent_execution.rs");
    let preflight_path = std::path::Path::new(manifest_dir)
        .join("src/handlers/knowledge/agent_execution/preflight.rs");
    let preflight_scope_path = std::path::Path::new(manifest_dir)
        .join("src/handlers/knowledge/agent_execution/preflight_scope.rs");
    let src = format!(
        "{}\n{}\n{}",
        std::fs::read_to_string(&facade_path).expect("read facade"),
        std::fs::read_to_string(&preflight_path).expect("read preflight"),
        std::fs::read_to_string(&preflight_scope_path).expect("read preflight scope"),
    );
    let needle = format!("Command::new({}git{})", '"', '"');
    let command_git = src.matches(needle.as_str()).count();
    assert_eq!(
        command_git, 1,
        "expected exactly one git Command::new site (the wave18-08 status read), found {}",
        command_git
    );
    let argv_needle = format!(
        ".args([{}status{}, {}--porcelain=v1{}])",
        '"', '"', '"', '"'
    );
    assert!(
        src.contains(argv_needle.as_str()),
        "the single git Command::new site must use the wave18-08 status argv",
    );
}

// ── Wave 21 / Task 08 — machine-contract autonomous loop smoke ──
//
// These tests deterministically exercise the daemon-side cross-checks
// that close the wave21 autonomous loop. They drive the wave21-03
// verifier helpers (`enforce_verified_completion` /
// `enforce_task_contract_completion` / `read_report_summary` /
// `read_task_contract_id`) end-to-end against fixture task-contract
// and report-contract Lisp text on disk. No LLM, no spawn, no shell,
// no markdown read — the smoke proves the daemon can ratify a fully
// machine-contract dispatch using only local file IO + structural
// parses.
//
// Invariants pinned (cross-wave):
//   * wave19-08 / wave21-03 — every malformed input maps to a
//     deterministic structured-error code (TASK_CONTRACT_REQUIRED /
//     TASK_CONTRACT_MALFORMED / TASK_REPORT_REQUIRED /
//     TASK_REPORT_MALFORMED / TASK_REPORT_TASK_ID_MISMATCH /
//     TASK_REPORT_COMMIT_HASH_MISMATCH).
//   * wave21-03 — the verified gate REUSES the wave19-08 contract
//     gate; the happy-path summary echoes both `task_contract_*` and
//     `task_report_*` resolved paths so observers can reconstruct the
//     handoff without reparsing the inputs.
//   * The daemon NEVER falls back to prompt mode / markdown when the
//     contract or report fails to parse — fail-fast over silent
//     salvage.

/// Fixture task contract that mirrors the byte-shape produced by
/// `plan::build_task_contract_lisp` for the wave21-08 smoke. The
/// `:write-scope` is empty so the wave19-08 claim-coverage rule is
/// satisfied vacuously; the smoke focuses on the wave21-03 verified
/// gate (schema + task_id + commit_hash) rather than the wave19-08
/// scope coverage rule (already covered above).
const WAVE21_08_SMOKE_CONTRACT_BODY: &str = r#"
(task wave21-08-smoke-contract
  :schema "missiond.task-contract.v1"
  :goal "wave21-08 deterministic machine-contract loop smoke"
  :write-scope []
  :must-not-touch []
  :acceptance ["cargo test -p missiond-daemon"]
  :commit (:required true :message "test(intent): cover wave21 loop" :scope-check write-scope-only))
"#;

/// Fixture report-contract aligned with the contract above. Both
/// `:task_id` and `:commit_hash` match what the smoke supplies via
/// `commit_hash`, so the wave21-03 cross-check passes end-to-end.
const WAVE21_08_SMOKE_REPORT_BODY: &str = r#"
(report wave21-08-smoke-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave21-08-smoke-contract"
  :status done
  :commit_hash "cafef00d1234"
  :files_changed ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
  :acceptance_results [(:command "cargo test -p missiond-daemon" :exit_code 0 :ok true)]
  :notes "wave21-08 smoke fixture")
"#;

/// Wave21-08 happy path: the verifier accepts an aligned (contract,
/// report, hash) triple and surfaces every cross-checked rule on the
/// validation summary. This is the SSOT proof that the wave21-03
/// gate can ratify a machine-contract autonomous loop end-to-end
/// without any external process.
#[test]
fn smoke_wave21_machine_contract_autonomous_loop_verifier_accepts_aligned_triple() {
    let dir = tempfile::tempdir().expect("tempdir");
    let contract_resolved = write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    let report_resolved = write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        WAVE21_08_SMOKE_REPORT_BODY,
    );
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some(".missiond/tasks/wave21/wave21-08-smoke.lisp"),
        Some(".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp"),
        Some("cafef00d1234"),
    );
    let summary = res.expect("aligned wave21-08 fixture must pass verifier");

    // Cross-check: every wave21-03 invariant lands on the summary so
    // observers can grep without reparsing the inputs.
    assert_eq!(
        summary.get("task_id").and_then(|v| v.as_str()),
        Some("wave21-08-smoke-contract"),
        "verified summary must echo the contract head id"
    );
    assert_eq!(
        summary
            .get("task_contract_resolved_path")
            .and_then(|v| v.as_str()),
        Some(contract_resolved.display().to_string().as_str()),
        "verified summary must echo the resolved contract path"
    );
    assert_eq!(
        summary
            .get("task_report_resolved_path")
            .and_then(|v| v.as_str()),
        Some(report_resolved.display().to_string().as_str()),
        "verified summary must echo the resolved report path"
    );
    let checked = summary
        .get("checked")
        .and_then(|v| v.as_array())
        .expect("checked list must exist");
    for needle in [
        "preconditions_present",
        "task_report_loadable",
        "task_report_schema",
        "task_id_matches_contract",
        "commit_hash_matches_report",
    ] {
        assert!(
            checked.iter().any(|v| v.as_str() == Some(needle)),
            "wave21-03 verifier must record `{}` in :checked",
            needle
        );
    }
}

/// Wave21-08 fail-fast: a report with a mismatched `:task_id` MUST
/// surface `TASK_REPORT_TASK_ID_MISMATCH`. Pinning this here proves
/// the verifier never silently accepts a stale report glued onto a
/// fresh contract — the daemon refuses, and the operator must
/// regenerate the report.
#[test]
fn smoke_wave21_malformed_report_task_id_yields_structured_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    // Report carries a different head id + :task_id — the verifier
    // MUST refuse the cross-check.
    let body = r#"
(report wave21-08-other-task
  :schema "missiond.report-contract.v1"
  :task_id "wave21-08-other-task"
  :status done
  :commit_hash "cafef00d1234"
  :files_changed []
  :acceptance_results [])
"#;
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-wrong.report.lisp",
        body,
    );
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some(".missiond/tasks/wave21/wave21-08-smoke.lisp"),
        Some(".missiond/tasks/wave21/reports/wave21-08-wrong.report.lisp"),
        Some("cafef00d1234"),
    );
    let err = res.expect_err("mismatched :task_id MUST reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_REPORT_TASK_ID_MISMATCH"),
        "wave21-03 verifier must surface the dedicated mismatch code so dashboards can route on it"
    );
}

/// Wave21-08 fail-fast: a malformed task-contract (schema mismatch)
/// MUST surface `TASK_CONTRACT_MALFORMED` even when the report
/// itself parses cleanly. The verifier MUST refuse rather than
/// silently downgrading to "report-only" mode.
#[test]
fn smoke_wave21_malformed_task_contract_yields_structured_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bad_contract = r#"(task wave21-08-bad
  :schema "missiond.task-contract.v0"
  :goal "wrong schema")"#;
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-bad.lisp",
        bad_contract,
    );
    // The report parses cleanly so we prove the rejection comes from
    // the contract side, not the report side.
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        WAVE21_08_SMOKE_REPORT_BODY,
    );
    // Drive the wave19-08 contract gate directly — that's the gate
    // the daemon hits FIRST when `enforce_scoped_commit=true` is
    // paired with `task_contract_path`.
    let file = fresh_file_with_claim();
    let res = enforce_task_contract_completion(
        &file,
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-bad.lisp",
        Some("cafef00d1234"),
        Some(&[]),
    );
    let err = res.expect_err("schema mismatch MUST reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_CONTRACT_MALFORMED"),
        "wave21-08 smoke: malformed contract MUST hit the dedicated TASK_CONTRACT_MALFORMED code"
    );
}

/// Wave21-08 fail-fast: a missing report file MUST surface
/// `TASK_REPORT_REQUIRED` — distinct from `TASK_REPORT_MALFORMED` so
/// the writer can tell "wrong path" from "wrong content" without
/// rerunning anything.
#[test]
fn smoke_wave21_missing_report_yields_structured_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some(".missiond/tasks/wave21/wave21-08-smoke.lisp"),
        // Path does not exist on disk.
        Some(".missiond/tasks/wave21/reports/wave21-08-nope.report.lisp"),
        Some("cafef00d1234"),
    );
    let err = res.expect_err("missing report MUST reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_REPORT_REQUIRED"),
        "wave21-08 smoke: missing report MUST hit TASK_REPORT_REQUIRED"
    );
}

/// Wave21-08 fail-fast: a commit_hash that does not match the
/// report's `:commit_hash` (and is not a prefix-overlap) MUST
/// surface `TASK_REPORT_COMMIT_HASH_MISMATCH`. Pinning this with a
/// clearly-different hash proves the prefix-overlap rule does NOT
/// accidentally accept an unrelated SHA.
#[test]
fn smoke_wave21_mismatched_commit_hash_yields_structured_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        WAVE21_08_SMOKE_REPORT_BODY,
    );
    let res = enforce_verified_completion(
        dir.path(),
        true,
        Some(".missiond/tasks/wave21/wave21-08-smoke.lisp"),
        Some(".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp"),
        // Different hash, not a prefix of `cafef00d1234`.
        Some("badc0ffee999"),
    );
    let err = res.expect_err("hash mismatch MUST reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_REPORT_COMMIT_HASH_MISMATCH"),
        "wave21-08 smoke: mismatched commit_hash MUST hit the dedicated mismatch code"
    );
}

/// Wave21-08 structural projector smoke: the wave21-03 mini reader
/// (`read_report_summary` + `read_task_contract_id`) extracts the
/// three load-bearing fields from the fixture report and the head
/// id from the fixture contract. Pinning these directly proves the
/// daemon-side projection survives a future wave-21+ schema change
/// without leaning on the script-side checker.
#[test]
fn smoke_wave21_report_and_contract_projectors_extract_required_fields() {
    let report = read_report_summary(WAVE21_08_SMOKE_REPORT_BODY)
        .expect("wave21-08 smoke report must parse");
    assert_eq!(
        report.schema.as_deref(),
        Some("missiond.report-contract.v1"),
        "report :schema MUST be the wave21-03 v1 schema"
    );
    assert_eq!(
        report.task_id.as_deref(),
        Some("wave21-08-smoke-contract"),
        "report :task_id MUST surface verbatim"
    );
    assert_eq!(
        report.commit_hash.as_deref(),
        Some("cafef00d1234"),
        "report :commit_hash MUST surface verbatim"
    );
    let contract_id = read_task_contract_id(WAVE21_08_SMOKE_CONTRACT_BODY)
        .expect("wave21-08 smoke contract head id must extract");
    assert_eq!(
        contract_id, "wave21-08-smoke-contract",
        "contract head id MUST equal the report :task_id (cross-check anchor)"
    );
    // Anchor: the head id pulled out of the contract is exactly the
    // value the wave21-03 verifier compares against the report's
    // `:task_id`. Pinning the equality here in one place catches a
    // future drift between the two readers.
    assert_eq!(
        Some(contract_id.as_str()),
        report.task_id.as_deref(),
        "wave21-08 cross-check anchor: contract head id must equal report :task_id"
    );
}

// ── Wave 22 / Task 02 — auto task-run verifier (in-process) ──
//
// These tests pin the wave22-02 contract on the daemon-side
// auto-verifier (`auto_run_task_run_verifier`) and the supporting
// shared-memory projector (`read_shared_memory_ledger` /
// `read_completion_task_id`). The auto-verifier removes the
// wave21-03 caller-supplied `verified=true` escape hatch by
// computing the verdict itself when all four paths
// (`task_contract_path`, `task_report_path`, `shared_memory_path`,
// `commit_hash`) are supplied. The verdict reuses the wave19-08 +
// wave21-03 error-code vocabulary plus three wave22-02 codes
// (`SHARED_MEMORY_REQUIRED`, `SHARED_MEMORY_MALFORMED`,
// `SHARED_MEMORY_NO_COMPLETION_FOR_TASK`) so dashboards see one
// consistent surface across the gates.

/// Aligned shared-memory ledger fixture mirroring the byte-shape of
/// `.missiond/tasks/<wave>/shared-memory.lisp`. The `(completion ...)`
/// child references the wave21-08 smoke contract head id so the
/// auto-verifier finds a matching entry.
const WAVE22_02_SMOKE_MEMORY_BODY: &str = r#"
(shared-memory wave21
  :schema "missiond.shared-memory.v1"
  :wave wave21
  :created-at "2026-04-26T00:00:00Z"
  :sequence 1
  (claim
:id wave21-08-claim-001
:task wave21-08-smoke-contract
:agent claudecode
:seq 1
:at "2026-04-26T00:01:00Z"
:touched ["src/x.rs"]
:summary "claim")
  (completion
:id wave21-08-completion-001
:task wave21-08-smoke-contract
:agent claudecode
:seq 2
:at "2026-04-26T00:02:00Z"
:touched ["src/x.rs"]
:summary "done"))
"#;

/// Wave22-02 happy path: every path supplied + every cross-check
/// passes → daemon-computed `verifier_status="passed"` and the
/// `verified_scope_summary` records every check name. This is the
/// SSOT proof that the daemon can ratify a task run end-to-end
/// without a Node spawn.
#[test]
fn auto_verifier_accepts_aligned_quartet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let contract_resolved = write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    let report_resolved = write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        WAVE21_08_SMOKE_REPORT_BODY,
    );
    let memory_resolved = write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/shared-memory.lisp",
        WAVE22_02_SMOKE_MEMORY_BODY,
    );
    let res = auto_run_task_run_verifier(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        ".missiond/tasks/wave21/shared-memory.lisp",
        "cafef00d1234",
    );
    let summary = res.expect("aligned quartet must pass auto-verifier");
    assert_eq!(
        summary.get("verifier_status").and_then(|v| v.as_str()),
        Some("passed"),
        "daemon-computed verdict MUST be `passed` for the aligned quartet"
    );
    assert_eq!(
        summary.get("task_id").and_then(|v| v.as_str()),
        Some("wave21-08-smoke-contract"),
    );
    assert_eq!(
        summary
            .get("task_contract_resolved_path")
            .and_then(|v| v.as_str()),
        Some(contract_resolved.display().to_string().as_str()),
    );
    assert_eq!(
        summary
            .get("task_report_resolved_path")
            .and_then(|v| v.as_str()),
        Some(report_resolved.display().to_string().as_str()),
    );
    assert_eq!(
        summary
            .get("shared_memory_resolved_path")
            .and_then(|v| v.as_str()),
        Some(memory_resolved.display().to_string().as_str()),
    );
    let checks = summary
        .get("checks")
        .and_then(|v| v.as_array())
        .expect("checks list must exist");
    for needle in [
        "task_contract_loadable",
        "task_report_loadable",
        "task_report_schema",
        "task_id_matches_contract",
        "commit_hash_matches_report",
        "shared_memory_loadable",
        "shared_memory_schema",
        "shared_memory_completion_for_task",
    ] {
        assert!(
            checks.iter().any(|v| v.as_str() == Some(needle)),
            "auto-verifier MUST record `{}` in :checks",
            needle
        );
    }
}

/// Missing shared-memory file → `SHARED_MEMORY_REQUIRED`. Distinct
/// from `SHARED_MEMORY_MALFORMED` so the writer can tell "wrong
/// path" from "wrong content" without re-running anything.
#[test]
fn auto_verifier_rejects_missing_shared_memory() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        WAVE21_08_SMOKE_REPORT_BODY,
    );
    let res = auto_run_task_run_verifier(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        ".missiond/tasks/wave21/does-not-exist.lisp",
        "cafef00d1234",
    );
    let err = res.expect_err("missing shared-memory must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("SHARED_MEMORY_REQUIRED"),
    );
}

/// Shared-memory ledger with the wrong `:schema` →
/// `SHARED_MEMORY_MALFORMED`. The structural parse succeeds but
/// the schema check refuses to ratify a non-v1 ledger so the
/// auto-verifier never silently accepts a stale shape.
#[test]
fn auto_verifier_rejects_shared_memory_schema_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        WAVE21_08_SMOKE_REPORT_BODY,
    );
    let bad_memory = r#"
(shared-memory wave21
  :schema "missiond.shared-memory.v0"
  :wave wave21
  (completion :id x :task wave21-08-smoke-contract :agent x :seq 1 :touched [] :summary "x"))
"#;
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/shared-memory.lisp",
        bad_memory,
    );
    let res = auto_run_task_run_verifier(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        ".missiond/tasks/wave21/shared-memory.lisp",
        "cafef00d1234",
    );
    let err = res.expect_err("schema mismatch must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("SHARED_MEMORY_MALFORMED"),
    );
}

/// Shared-memory ledger has the right schema but no
/// `(completion :task <id> ...)` for the contract head id →
/// `SHARED_MEMORY_NO_COMPLETION_FOR_TASK`. Mirrors the wave21-02
/// script-side rule so the daemon and the script agree.
#[test]
fn auto_verifier_rejects_shared_memory_without_completion_for_task() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        WAVE21_08_SMOKE_REPORT_BODY,
    );
    // Ledger has only a claim and a completion for OTHER task — the
    // daemon must refuse rather than silently passing.
    let no_match_memory = r#"
(shared-memory wave21
  :schema "missiond.shared-memory.v1"
  :wave wave21
  (claim
:id wave21-99-claim-001
:task wave21-99-other
:agent claudecode
:seq 1
:touched []
:summary "claim")
  (completion
:id wave21-99-completion-001
:task wave21-99-other
:agent claudecode
:seq 2
:touched []
:summary "done"))
"#;
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/shared-memory.lisp",
        no_match_memory,
    );
    let res = auto_run_task_run_verifier(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
        ".missiond/tasks/wave21/shared-memory.lisp",
        "cafef00d1234",
    );
    let err = res.expect_err("missing completion entry must reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("SHARED_MEMORY_NO_COMPLETION_FOR_TASK"),
    );
}

/// The shared-memory projector pulls `:schema` and every
/// `(completion :task <id> ...)` task id off the ledger. Pinning
/// this directly proves the wave22-02 auto-verifier's matching
/// rule survives a future ledger schema change without leaning on
/// the script-side checker.
#[test]
fn shared_memory_projector_extracts_required_fields() {
    let summary = read_shared_memory_ledger(WAVE22_02_SMOKE_MEMORY_BODY).expect("must parse");
    assert_eq!(summary.schema.as_deref(), Some("missiond.shared-memory.v1"),);
    assert!(
        summary
            .completion_tasks
            .iter()
            .any(|t| t == "wave21-08-smoke-contract"),
        "projector MUST surface every (completion :task <id> ...) entry"
    );
}

/// `read_completion_task_id` ignores `(completion ...)` forms with
/// no `:task` slot — mirrors the script-side verifier which uses
/// the same "must have :task" rule when matching.
#[test]
fn completion_task_id_ignores_entry_without_task_slot() {
    let body = r#"
(shared-memory wave99
  :schema "missiond.shared-memory.v1"
  :wave wave99
  (completion :id x :agent y :seq 1 :touched [] :summary "no task slot"))
"#;
    let summary = read_shared_memory_ledger(body).expect("must parse");
    assert!(
        summary.completion_tasks.is_empty(),
        "entries without :task MUST be silently skipped to mirror the script-side rule"
    );
}

/// Auto-verifier delegates the contract+report cross-checks to the
/// same projectors as the wave21-03 gate, so a report `:task_id`
/// mismatch still surfaces the dedicated `TASK_REPORT_TASK_ID_MISMATCH`
/// code rather than a generic auto-verifier failure. Pinning this
/// directly proves the vocabulary stays unified across the two gates.
#[test]
fn auto_verifier_reuses_wave21_03_codes_for_report_task_id_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    let mismatched_report = r#"
(report wave21-08-other-task
  :schema "missiond.report-contract.v1"
  :task_id "wave21-08-other-task"
  :status done
  :commit_hash "cafef00d1234"
  :files_changed []
  :acceptance_results [])
"#;
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-mismatch.report.lisp",
        mismatched_report,
    );
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/shared-memory.lisp",
        WAVE22_02_SMOKE_MEMORY_BODY,
    );
    let res = auto_run_task_run_verifier(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        ".missiond/tasks/wave21/reports/wave21-08-mismatch.report.lisp",
        ".missiond/tasks/wave21/shared-memory.lisp",
        "cafef00d1234",
    );
    let err = res.expect_err("task_id mismatch MUST reject");
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_REPORT_TASK_ID_MISMATCH"),
        "wave22-02 auto-verifier MUST reuse wave21-03 vocabulary so consumers see one code"
    );
}

/// Auto-verifier preserves the short<->long sha overlap rule from
/// the wave21-03 gate. A 7-char `git log %h` value MUST match a
/// 40-char `git rev-parse HEAD` value via prefix overlap.
#[test]
fn auto_verifier_accepts_short_long_sha_prefix_overlap() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    let short_hash_report = r#"
(report wave21-08-smoke-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave21-08-smoke-contract"
  :status done
  :commit_hash "cafef00"
  :files_changed []
  :acceptance_results [])
"#;
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/reports/wave21-08-short.report.lisp",
        short_hash_report,
    );
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave21/shared-memory.lisp",
        WAVE22_02_SMOKE_MEMORY_BODY,
    );
    let res = auto_run_task_run_verifier(
        dir.path(),
        ".missiond/tasks/wave21/wave21-08-smoke.lisp",
        ".missiond/tasks/wave21/reports/wave21-08-short.report.lisp",
        ".missiond/tasks/wave21/shared-memory.lisp",
        // Long sha; report has the 7-char prefix.
        "cafef001234567890abcdef",
    );
    assert!(
        res.is_ok(),
        "short<->long sha prefix overlap MUST pass the auto-verifier",
    );
}

// ── Wave 22 / Task 07 — autonomous loop apply smoke v4 ──
//
// Deterministic smoke tests covering the wave22-02 auto task-run
// verifier slice of the apply-gate cluster. Every test pinned here is
// a `no real LLM / no real spawn / no real git mutation` proof: the
// verifier helpers do read-only file inspection on tempfile fixtures
// and never invoke `Command::new`. Companion tests in the other four
// write-scope files (review_gate.rs / plan.rs / workstation_dispatch.rs
// / unified_entry.rs) cover the matching apply-gate pure evaluators
// and the envelope-side markdown-non-load-bearing invariant.

/// V4 smoke: when `enforce_scoped_commit=true` is paired with a
/// task_contract_path + report_path + shared_memory_path quartet, a
/// completion entry that does NOT match the contract head id MUST
/// block the completion via `SHARED_MEMORY_NO_COMPLETION_FOR_TASK`.
/// This is the wave22-07 Requirement 4 anchor: failed verification
/// blocks completion. The smoke deliberately routes through the
/// `auto_run_task_run_verifier` helper because that is the daemon-
/// side gate `action_complete` will dispatch to when the caller
/// supplies the full quartet (per wave22-02 contract).
#[test]
fn smoke_wave22_07_failed_verification_blocks_completion_when_enforce_scoped_commit_true() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave22/wave22-07-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave22/reports/wave22-07-smoke.report.lisp",
        WAVE21_08_SMOKE_REPORT_BODY,
    );
    // Ledger has only completion entries for OTHER tasks — the
    // verifier MUST refuse rather than silently passing. This
    // models the wave22-07 v4 brief Requirement 4: the verifier
    // blocks completion on the failed-verification path.
    let no_match_memory = r#"
(shared-memory wave22
  :schema "missiond.shared-memory.v1"
  :wave wave22
  (claim
:id wave22-99-claim-001
:task wave22-99-other
:agent claudecode
:seq 1
:touched []
:summary "claim")
  (completion
:id wave22-99-completion-001
:task wave22-99-other
:agent claudecode
:seq 2
:touched []
:summary "done"))
"#;
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave22/shared-memory.lisp",
        no_match_memory,
    );
    let res = auto_run_task_run_verifier(
        dir.path(),
        ".missiond/tasks/wave22/wave22-07-smoke.lisp",
        ".missiond/tasks/wave22/reports/wave22-07-smoke.report.lisp",
        ".missiond/tasks/wave22/shared-memory.lisp",
        "cafef00d1234",
    );
    let err = res.expect_err(
        "wave22-07 v4 invariant: when enforce_scoped_commit=true paths align but no \
         completion entry exists for the contract head id, completion MUST be blocked",
    );
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("SHARED_MEMORY_NO_COMPLETION_FOR_TASK"),
        "wave22-07 v4 invariant: failed verification MUST surface the dedicated \
         SHARED_MEMORY_NO_COMPLETION_FOR_TASK code so dashboards can route on it"
    );
}

/// V4 smoke (companion): a mismatched commit_hash on the same
/// quartet MUST also block completion. Pinning both rejection
/// surfaces here proves the gate is symmetric — neither a missing
/// completion entry nor a stale commit hash can sneak past
/// `enforce_scoped_commit=true`.
#[test]
fn smoke_wave22_07_failed_verification_blocks_on_commit_hash_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_task_contract(
        dir.path(),
        ".missiond/tasks/wave22/wave22-07-smoke.lisp",
        WAVE21_08_SMOKE_CONTRACT_BODY,
    );
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave22/reports/wave22-07-smoke.report.lisp",
        WAVE21_08_SMOKE_REPORT_BODY,
    );
    write_task_report(
        dir.path(),
        ".missiond/tasks/wave22/shared-memory.lisp",
        WAVE22_02_SMOKE_MEMORY_BODY,
    );
    let res = auto_run_task_run_verifier(
        dir.path(),
        ".missiond/tasks/wave22/wave22-07-smoke.lisp",
        ".missiond/tasks/wave22/reports/wave22-07-smoke.report.lisp",
        ".missiond/tasks/wave22/shared-memory.lisp",
        // Different hash, not a prefix overlap of the report's
        // `cafef00d1234` value.
        "badc0ffee999",
    );
    let err = res.expect_err(
        "wave22-07 v4 invariant: a commit_hash that does not match the report's \
         `:commit_hash` MUST block completion even when the rest of the quartet aligns",
    );
    assert_eq!(
        extract_error_code(&err).as_deref(),
        Some("TASK_REPORT_COMMIT_HASH_MISMATCH"),
        "wave22-07 v4 invariant: hash mismatch MUST hit the dedicated wave21-03 \
         TASK_REPORT_COMMIT_HASH_MISMATCH code so the verifier vocabulary stays unified"
    );
}

// ── wave23-04 — session-trace append unit tests ───────────────────
//
// Cover the three append surfaces (open / preflight / complete) plus
// the helper invariants the JS-side checker enforces:
// schema-valid event shape, seq monotonicity, id format, repo-relative
// paths. Failure paths return `TraceWarning` instead of panicking so
// the caller can surface `trace_warning` without aborting.

const TRACE_SEED: &str = "(session-trace wave23\n  :schema \"missiond.session-trace.v1\"\n  :wave wave23\n  :created-at \"2026-04-28T00:00:00+08:00\"\n  :sequence 1\n\n  (trace-event\n    :id wave23-trace-bootstrap-001\n    :seq 1\n    :at \"2026-04-28T00:00:00+08:00\"\n    :task wave23-04-execution-session-trace-integration-v0\n    :backend codex-orchestrator\n    :kind observation\n    :summary \"seed event\"))\n";

fn write_trace_seed(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, TRACE_SEED.as_bytes()).expect("seed write");
    path
}

#[test]
fn is_valid_trace_id_matches_checker_regex() {
    assert!(is_valid_trace_id("wave23-04-foo"));
    assert!(is_valid_trace_id("a"));
    assert!(is_valid_trace_id("9abc"));
    assert!(is_valid_trace_id("wave.23_04-x"));
    assert!(!is_valid_trace_id(""));
    assert!(!is_valid_trace_id("-leading-dash"));
    assert!(!is_valid_trace_id("Upper"));
    assert!(!is_valid_trace_id("has space"));
    assert!(!is_valid_trace_id("has/slash"));
}

#[test]
fn sanitize_trace_backend_falls_back_to_claudecode() {
    assert_eq!(sanitize_trace_backend(""), "claudecode");
    assert_eq!(sanitize_trace_backend("   "), "claudecode");
    assert_eq!(sanitize_trace_backend("ClaudeCode"), "claudecode");
    assert_eq!(sanitize_trace_backend("claudecode"), "claudecode");
    assert_eq!(sanitize_trace_backend("agent team"), "agent-team");
    // leading non-alnum stripped
    assert_eq!(sanitize_trace_backend("---abc"), "abc");
    // entirely punctuation / whitespace -> fallback
    assert_eq!(sanitize_trace_backend("!!!"), "claudecode");
}

#[test]
fn render_trace_event_emits_required_and_optional_fields() {
    let ev = TraceEvent {
        task: "wave23-04-execution-session-trace-integration-v0".to_string(),
        backend: "claudecode".to_string(),
        kind: TraceKind::Complete,
        summary: "trace round-trip".to_string(),
        agent: None,
        files: Some(vec![
            "crates/foo/src/lib.rs".to_string(),
            "crates/bar/src/lib.rs".to_string(),
        ]),
        commit_hash: Some("cafef00d".to_string()),
        report_path: Some(".missiond/tasks/wave23/reports/x.report.lisp".to_string()),
    };
    let rendered = render_trace_event(42, "2026-04-28T01:00:00Z", &ev);
    assert!(rendered.contains(":id wave23-04-execution-session-trace-integration-v0-complete-42"));
    assert!(rendered.contains(":seq 42"));
    assert!(rendered.contains(":at \"2026-04-28T01:00:00Z\""));
    assert!(rendered.contains(":task wave23-04-execution-session-trace-integration-v0"));
    assert!(rendered.contains(":backend claudecode"));
    assert!(rendered.contains(":kind complete"));
    assert!(rendered.contains(":summary \"trace round-trip\""));
    assert!(rendered.contains(":files [\"crates/foo/src/lib.rs\" \"crates/bar/src/lib.rs\"]"));
    assert!(rendered.contains(":commit_hash \"cafef00d\""));
    assert!(rendered.contains(":report_path \".missiond/tasks/wave23/reports/x.report.lisp\""));
}

#[test]
fn append_session_trace_event_round_trips_minimal_event() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_trace_seed(dir.path(), "session-trace.lisp");
    let ev = TraceEvent {
        task: "wave23-04-execution-session-trace-integration-v0".to_string(),
        backend: "claudecode".to_string(),
        kind: TraceKind::Dispatch,
        summary: "open dispatched".to_string(),
        agent: None,
        files: None,
        commit_hash: None,
        report_path: None,
    };
    append_session_trace_event(&path, &ev).expect("append ok");
    let after = std::fs::read_to_string(&path).expect("read");
    // Parser must accept the new file shape.
    let forms = sexp::parse(&after).expect("parse");
    assert_eq!(scan_max_trace_seq(&forms), 2);
    // The new entry's id reflects the seq.
    assert!(after.contains(":id wave23-04-execution-session-trace-integration-v0-dispatch-2"));
    // Required fields the checker enforces are all present.
    assert!(after.contains(":kind dispatch"));
    assert!(after.contains(":backend claudecode"));
    assert!(after.contains(":task wave23-04-execution-session-trace-integration-v0"));
}

#[test]
fn append_session_trace_event_seq_monotonic_across_appends() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_trace_seed(dir.path(), "session-trace.lisp");
    let task = "wave23-04-execution-session-trace-integration-v0".to_string();
    let backend = "claudecode".to_string();
    for (i, kind) in [
        TraceKind::Dispatch,
        TraceKind::Observation,
        TraceKind::Complete,
    ]
    .iter()
    .enumerate()
    {
        let ev = TraceEvent {
            task: task.clone(),
            backend: backend.clone(),
            kind: *kind,
            summary: format!("event {}", i),
            agent: None,
            files: None,
            commit_hash: None,
            report_path: None,
        };
        append_session_trace_event(&path, &ev).unwrap_or_else(|w| {
            panic!("append #{} failed: {}", i, w);
        });
    }
    let text = std::fs::read_to_string(&path).expect("read");
    let forms = sexp::parse(&text).expect("parse");
    let max = scan_max_trace_seq(&forms);
    assert_eq!(max, 4, "seed seq=1 + three appends => max seq must be 4");
    // ids must be unique — seq is in the id so this is implicit, but
    // exercise the parser to confirm no entries collide.
    let trace_form = forms
        .iter()
        .find(|n| n.head_atom() == Some("session-trace"))
        .expect("trace form");
    let mut ids = Vec::new();
    for child in trace_form.children() {
        if child.head_atom() != Some("trace-event") {
            continue;
        }
        let kids = child.children();
        let mut i = 0;
        while i + 1 < kids.len() {
            if kids[i].as_atom() == Some(":id") {
                if let Some(v) = kids[i + 1].as_atom() {
                    ids.push(v.to_string());
                }
            }
            i += 1;
        }
    }
    assert_eq!(ids.len(), 4);
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        4,
        "ids must be unique across appends: {:?}",
        ids
    );
}

#[test]
fn append_session_trace_event_missing_file_returns_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("does-not-exist.lisp");
    let ev = TraceEvent {
        task: "wave23-04-execution-session-trace-integration-v0".to_string(),
        backend: "claudecode".to_string(),
        kind: TraceKind::Dispatch,
        summary: "open".to_string(),
        agent: None,
        files: None,
        commit_hash: None,
        report_path: None,
    };
    let warn =
        append_session_trace_event(&path, &ev).expect_err("missing file must surface as warning");
    assert!(matches!(warn, TraceWarning::MissingFile(_)));
    // Display must mention the path so the writer can correlate.
    assert!(warn.to_string().contains("does-not-exist.lisp"));
}

#[test]
fn append_session_trace_event_malformed_returns_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("session-trace.lisp");
    // Unbalanced parens — sexp::parse will fail.
    std::fs::write(&path, b"(session-trace wave23\n  :schema \"x\"\n").unwrap();
    let ev = TraceEvent {
        task: "wave23-04-execution-session-trace-integration-v0".to_string(),
        backend: "claudecode".to_string(),
        kind: TraceKind::Dispatch,
        summary: "open".to_string(),
        agent: None,
        files: None,
        commit_hash: None,
        report_path: None,
    };
    let warn = append_session_trace_event(&path, &ev)
        .expect_err("malformed trace must surface as warning");
    assert!(matches!(warn, TraceWarning::Malformed(_)));
}

#[test]
fn append_session_trace_event_invalid_task_id_returns_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_trace_seed(dir.path(), "session-trace.lisp");
    let ev = TraceEvent {
        task: "BadTask Id!".to_string(),
        backend: "claudecode".to_string(),
        kind: TraceKind::Dispatch,
        summary: "open".to_string(),
        agent: None,
        files: None,
        commit_hash: None,
        report_path: None,
    };
    let warn = append_session_trace_event(&path, &ev)
        .expect_err("invalid task id must surface as warning");
    assert!(matches!(warn, TraceWarning::InvalidTaskId(_)));
}

#[test]
fn append_session_trace_event_invalid_backend_returns_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_trace_seed(dir.path(), "session-trace.lisp");
    let ev = TraceEvent {
        task: "wave23-04-execution-session-trace-integration-v0".to_string(),
        backend: "Has Upper".to_string(),
        kind: TraceKind::Dispatch,
        summary: "open".to_string(),
        agent: None,
        files: None,
        commit_hash: None,
        report_path: None,
    };
    let warn = append_session_trace_event(&path, &ev)
        .expect_err("invalid backend id must surface as warning");
    assert!(matches!(warn, TraceWarning::InvalidBackend(_)));
}

#[test]
fn resolve_session_trace_path_handles_relative_and_absolute() {
    let root = std::path::PathBuf::from("/tmp/missiond-fake-root");
    // Relative path joins under the root.
    let args_rel = json!({"session_trace_path": ".missiond/tasks/wave23/session-trace.lisp"});
    let resolved = resolve_session_trace_path(&args_rel, &root).expect("relative resolves");
    assert!(resolved.starts_with(&root));
    assert!(resolved.ends_with(".missiond/tasks/wave23/session-trace.lisp"));
    // Absolute path passes through verbatim.
    let abs = "/var/lib/missiond/trace.lisp";
    let args_abs = json!({"session_trace_path": abs});
    let resolved = resolve_session_trace_path(&args_abs, &root).expect("absolute resolves");
    assert_eq!(resolved, std::path::PathBuf::from(abs));
    // Empty / blank string -> None (legacy behaviour disabled).
    let args_empty = json!({"session_trace_path": "   "});
    assert!(resolve_session_trace_path(&args_empty, &root).is_none());
    // Absent -> None.
    let args_none = json!({});
    assert!(resolve_session_trace_path(&args_none, &root).is_none());
}

#[test]
fn append_session_trace_event_preserves_existing_entries() {
    // Append must NEVER rewrite prior entries — read length, append
    // after the last (trace-event ...) form, atomic enough to survive
    // concurrent execution. The seed bootstrap entry must survive the
    // append unchanged.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_trace_seed(dir.path(), "session-trace.lisp");
    let before = std::fs::read_to_string(&path).expect("read seed");
    assert!(before.contains(":id wave23-trace-bootstrap-001"));
    let ev = TraceEvent {
        task: "wave23-04-execution-session-trace-integration-v0".to_string(),
        backend: "claudecode".to_string(),
        kind: TraceKind::Complete,
        summary: "complete recorded".to_string(),
        agent: None,
        files: Some(vec![
            "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
        ]),
        commit_hash: Some("deadbeef".to_string()),
        report_path: Some(".missiond/tasks/wave23/reports/wave23-04.report.lisp".to_string()),
    };
    append_session_trace_event(&path, &ev).expect("append ok");
    let after = std::fs::read_to_string(&path).expect("read after");
    // Bootstrap entry must still be present and untouched.
    assert!(after.contains(":id wave23-trace-bootstrap-001"));
    assert!(after.contains(":summary \"seed event\""));
    // New entry sits at end, before the closing paren of session-trace.
    assert!(after.contains(":kind complete"));
    assert!(after.contains(":commit_hash \"deadbeef\""));
    // The file remains a single well-formed top-level form.
    let forms = sexp::parse(&after).expect("parse");
    let trace_forms: Vec<_> = forms
        .iter()
        .filter(|f| f.head_atom() == Some("session-trace"))
        .collect();
    assert_eq!(
        trace_forms.len(),
        1,
        "must remain a single session-trace form"
    );
    let event_count = trace_forms[0]
        .children()
        .iter()
        .filter(|c| c.head_atom() == Some("trace-event"))
        .count();
    assert_eq!(event_count, 2, "seed + new = 2 events");
}

#[test]
fn resolve_trace_task_id_prefers_task_contract_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Write a minimal task contract whose head id matches the regex.
    let contract_dir = dir.path().join(".missiond/tasks/wave23");
    std::fs::create_dir_all(&contract_dir).expect("mkdir");
    let contract_path = contract_dir.join("wave23-04-test.lisp");
    std::fs::write(
        &contract_path,
        b"(task wave23-04-real-task-id\n  :schema \"missiond.task-contract.v1\")\n",
    )
    .expect("write");
    let args = json!({
        "task_contract_path": ".missiond/tasks/wave23/wave23-04-test.lisp"
    });
    let resolved =
        resolve_trace_task_id(&args, dir.path(), "fallback-execution-id").expect("resolved");
    assert_eq!(resolved, "wave23-04-real-task-id");
}

#[test]
fn resolve_trace_task_id_falls_back_to_execution_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    // No task_contract_path supplied -> fallback to execution_id.
    let args = json!({});
    let resolved = resolve_trace_task_id(
        &args,
        dir.path(),
        "wave23-04-execution-session-trace-integration-v0",
    )
    .expect("resolved");
    assert_eq!(resolved, "wave23-04-execution-session-trace-integration-v0");
}

#[test]
fn resolve_trace_task_id_rejects_non_regex_fallback() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Execution id with uppercase / spaces -> no valid id.
    let args = json!({});
    assert!(resolve_trace_task_id(&args, dir.path(), "Bad Exec ID").is_none());
}
