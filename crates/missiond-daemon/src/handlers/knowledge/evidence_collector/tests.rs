//! Regression tests for the evidence_collector surface.

use super::*;

// ── basic builder shape ───────────────────────────────────────────

#[test]
fn entry_always_carries_canonical_stamps() {
    let v = EvidenceEntry::new(source::RECORD_EVIDENCE_MANUAL, kind::NOTE).into_json();
    let obj = v.as_object().expect("entry is object");
    assert_eq!(
        obj.get("schema_version").and_then(|v| v.as_str()),
        Some(EVIDENCE_SCHEMA_VERSION),
        "schema_version always present"
    );
    assert_eq!(
        obj.get("source").and_then(|v| v.as_str()),
        Some(source::RECORD_EVIDENCE_MANUAL),
    );
    assert_eq!(obj.get("kind").and_then(|v| v.as_str()), Some(kind::NOTE));
}

#[test]
fn typed_setters_land_under_canonical_keys() {
    let entry = EvidenceEntry::new(source::PLAN_RUNNER_DISPATCH, kind::DISPATCH)
        .with_inner_dispatch(json!({"status": "executing"}))
        .with_verification(
            vec!["cargo test".to_string(), "cargo build".to_string()],
            json!({"passed": true}),
        )
        .with_git_diff(json!({"files_changed": 3, "insertions": 10}))
        .with_commit(Some("abc1234".to_string()), Some("committed".to_string()))
        .with_state_transition("ready -> succeeded");
    let v = entry.into_json();
    let obj = v.as_object().unwrap();
    assert_eq!(obj.get("inner_dispatch").unwrap()["status"], "executing");
    assert_eq!(
        obj.get("verification").unwrap()["commands"][0],
        "cargo test"
    );
    assert_eq!(
        obj.get("verification").unwrap()["result_summary"]["passed"],
        true
    );
    assert_eq!(obj.get("git_diff").unwrap()["files_changed"], 3);
    assert_eq!(obj.get("commit").unwrap()["commit_hash"], "abc1234");
    assert_eq!(obj.get("commit").unwrap()["commit_status"], "committed");
    assert_eq!(obj.get("state_transition").unwrap(), "ready -> succeeded");
}

#[test]
fn commit_setter_skipped_when_both_fields_none() {
    let entry = EvidenceEntry::new("test", "test").with_commit(None, None);
    let v = entry.into_json();
    assert!(
        v.as_object().unwrap().get("commit").is_none(),
        "commit key omitted when both hash and status are None"
    );
}

#[test]
fn commit_setter_keeps_partial_metadata() {
    // Only one of the two fields supplied — should still record what we have.
    let entry = EvidenceEntry::new("test", "test").with_commit(None, Some("dirty".to_string()));
    let v = entry.into_json();
    let commit = v.as_object().unwrap().get("commit").expect("commit kept");
    assert_eq!(commit["commit_status"], "dirty");
    assert!(
        commit.get("commit_hash").is_none(),
        "absent hash stays absent"
    );
}

// ── execution event refs ──────────────────────────────────────────

#[test]
fn execution_events_array_in_order() {
    let entry = EvidenceEntry::new("test", kind::DISPATCH)
        .add_execution_event(EventRef::new("mission_execution", "opened", "C001"))
        .add_execution_event(EventRef::new("mission_execution", "completed", "COMP005"));
    let v = entry.into_json();
    let arr = v["execution_events"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["event_id"], "C001");
    assert_eq!(arr[0]["source"], "mission_execution");
    assert_eq!(arr[0]["kind"], "opened");
    assert_eq!(arr[1]["event_id"], "COMP005");
}

#[test]
fn execution_event_unavailable_records_reason_not_id() {
    let entry = EvidenceEntry::new("test", kind::DISPATCH)
        .add_execution_event(EventRef::unavailable("event ref unavailable by policy"));
    let v = entry.into_json();
    let arr = v["execution_events"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["unavailable"], true);
    assert_eq!(
        arr[0]["unavailable_reason"],
        "event ref unavailable by policy"
    );
    assert_eq!(arr[0]["status"], "unavailable");
    assert!(arr[0].get("event_id").is_none());
}

#[test]
fn execution_events_omitted_when_none_attached() {
    let v = EvidenceEntry::new("test", kind::NOTE).into_json();
    assert!(
        v.as_object().unwrap().get("execution_events").is_none(),
        "no events attached → key absent (don't pretend with empty array)"
    );
}

// ── wave-16 / task 07 :: EventRefStatus surface ────────────────────

#[test]
fn event_ref_live_status_round_trips_via_new_alias() {
    // `EventRef::new(...)` is the wave-13/14 alias for `live(...)`. It
    // must continue to mark the resulting envelope with `status=live`
    // so existing publish-path call sites (plan_dag) get the right
    // provenance tag for free.
    let r = EventRef::new("execution", "plan_node_state_changed", "42");
    assert_eq!(r.status, EventRefStatus::Live);
    let json = EvidenceEntry::new("test", kind::DISPATCH)
        .add_execution_event(r)
        .into_json();
    let arr = json["execution_events"].as_array().unwrap();
    assert_eq!(arr[0]["status"], "live");
    assert_eq!(arr[0]["event_id"], "42");
}

#[test]
fn event_ref_log_status_round_trips() {
    // Resolved post-hoc via the resolver — should surface as `log`.
    let r = EventRef::from_log("execution", "plan_node_state_changed", "99");
    assert_eq!(r.status, EventRefStatus::Log);
    let json = EvidenceEntry::new("test", kind::DISPATCH)
        .add_execution_event(r)
        .into_json();
    let arr = json["execution_events"].as_array().unwrap();
    assert_eq!(arr[0]["status"], "log");
    assert_eq!(arr[0]["event_id"], "99");
}

// ── wave-16 / task 07 :: EventRefResolver ──────────────────────────

#[test]
fn resolver_returns_log_ref_on_cache_hit() {
    let resolver = EventRefResolver::new();
    let plan_id = "11111111-1111-1111-1111-111111111111";
    resolver.record_plan_node_state_change(
        plan_id,
        "n1",
        1,
        "ready",
        "running",
        "execution",
        "plan_node_state_changed",
        "777",
    );
    let r = resolver.lookup_plan_node_state_change(plan_id, "n1", 1, "ready", "running");
    assert_eq!(r.status, EventRefStatus::Log);
    assert_eq!(r.event_id.as_deref(), Some("777"));
    assert_eq!(r.source.as_deref(), Some("execution"));
    assert_eq!(r.kind.as_deref(), Some("plan_node_state_changed"));
    assert!(r.unavailable_reason.is_none());
}

#[test]
fn resolver_returns_unavailable_on_cache_miss() {
    let resolver = EventRefResolver::new();
    let r = resolver.lookup_plan_node_state_change(
        "00000000-0000-0000-0000-000000000000",
        "missing-node",
        1,
        "ready",
        "succeeded",
    );
    assert_eq!(r.status, EventRefStatus::Unavailable);
    assert_eq!(
        r.unavailable_reason.as_deref(),
        Some(EVENT_REF_RESOLVER_MISS_REASON),
        "miss reason kept stable so audit consumers can pivot on it"
    );
    assert!(r.event_id.is_none());
}

#[test]
fn resolver_evicts_oldest_when_capacity_reached() {
    // Cap=2 — third insert evicts the first.
    let resolver = EventRefResolver::with_capacity(2);
    for (node, seq) in [("n1", "1"), ("n2", "2"), ("n3", "3")] {
        resolver.record_plan_node_state_change(
            "p",
            node,
            1,
            "ready",
            "running",
            "execution",
            "plan_node_state_changed",
            seq,
        );
    }
    // n1 evicted; n2 + n3 retained.
    let r1 = resolver.lookup_plan_node_state_change("p", "n1", 1, "ready", "running");
    assert_eq!(r1.status, EventRefStatus::Unavailable, "first key evicted");
    let r2 = resolver.lookup_plan_node_state_change("p", "n2", 1, "ready", "running");
    assert_eq!(r2.event_id.as_deref(), Some("2"));
    let r3 = resolver.lookup_plan_node_state_change("p", "n3", 1, "ready", "running");
    assert_eq!(r3.event_id.as_deref(), Some("3"));
    assert_eq!(resolver.len(), 2);
}

#[test]
fn resolver_reinsert_overwrites_value_without_double_counting_order() {
    // Re-recording the same key should overwrite the value but NOT add
    // a second slot to the eviction order — otherwise a hot key would
    // squeeze cold ones out of the cap prematurely.
    let resolver = EventRefResolver::with_capacity(2);
    for seq in ["1", "2"] {
        resolver.record_plan_node_state_change(
            "p",
            "n1",
            1,
            "ready",
            "running",
            "execution",
            "plan_node_state_changed",
            seq,
        );
    }
    // n1 still occupies one slot; one more slot is free.
    resolver.record_plan_node_state_change(
        "p",
        "n2",
        1,
        "ready",
        "running",
        "execution",
        "plan_node_state_changed",
        "X",
    );
    // Both entries must survive.
    let r1 = resolver.lookup_plan_node_state_change("p", "n1", 1, "ready", "running");
    assert_eq!(
        r1.event_id.as_deref(),
        Some("2"),
        "overwrite kept newest value"
    );
    let r2 = resolver.lookup_plan_node_state_change("p", "n2", 1, "ready", "running");
    assert_eq!(r2.event_id.as_deref(), Some("X"));
    assert_eq!(resolver.len(), 2);
}

#[test]
fn resolver_key_format_matches_deterministic_event_id() {
    // The cache key MUST match the deterministic event id format used
    // by `plan_dag::deterministic_plan_node_event_id` so cache lookups
    // can pivot on the same correlation tuple.
    let key = EventRefResolver::plan_node_state_change_key(
        "11111111-1111-1111-1111-111111111111",
        "node-A",
        3,
        "running",
        "succeeded",
    );
    assert_eq!(
        key,
        "plan-node:11111111-1111-1111-1111-111111111111:node-A:3:running-succeeded"
    );
}

// ── wave-17 / task 06 :: log-query path ─────────────────────────────

/// Stub `LogReadable` that returns a caller-supplied list of rows on
/// every `read_from`. Lets us exercise the log-query branch of
/// [`EventRefResolver::lookup_or_query_plan_node_state_change`] without
/// standing up a real PG `LogWriter`. The stub is `Send + Sync` and
/// tracks the call count so tests can assert that a cache hit DOES
/// NOT trigger a log scan.
#[derive(Default)]
struct StubLog {
    rows: Vec<missiond_core::event::log::LoggedEvent>,
    err: Option<String>,
    calls: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl missiond_core::event::log::LogReadable for StubLog {
    async fn read_from(
        &self,
        _domain: missiond_core::event::Domain,
        _after: missiond_core::event::log::Seq,
        _limit: usize,
    ) -> Result<Vec<missiond_core::event::log::LoggedEvent>, missiond_core::event::log::LogError>
    {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(e) = &self.err {
            return Err(missiond_core::event::log::LogError::Other(e.clone()));
        }
        Ok(self.rows.clone())
    }
    async fn head_seq(
        &self,
    ) -> Result<missiond_core::event::log::Seq, missiond_core::event::log::LogError> {
        Ok(missiond_core::event::log::Seq(
            self.rows.last().map(|r| r.seq.0).unwrap_or(0),
        ))
    }
}

fn fixture_plan_node_logged_event(
    seq: i64,
    plan_id: &str,
    node_id: &str,
    attempt: Option<u32>,
    from: &str,
    to: &str,
) -> missiond_core::event::log::LoggedEvent {
    let payload = json!({
        "PlanNodeStateChanged": {
            "plan_id": plan_id,
            "node_id": node_id,
            "from": from,
            "to": to,
            "attempt": attempt,
        }
    });
    missiond_core::event::log::LoggedEvent {
        seq: missiond_core::event::log::Seq(seq),
        domain: missiond_core::event::Domain::Execution,
        kind: "plan_node_state_changed".to_string(),
        payload,
        producer_id: "test/plan_dag".to_string(),
        dedupe_key: None,
        causation_depth: 0,
        trace_id: None,
        span_id: None,
        parent_span_id: None,
        ts: chrono::Utc::now(),
        ephemeral: false,
    }
}

#[tokio::test]
async fn resolver_log_query_returns_log_ref_after_cache_miss() {
    let resolver = EventRefResolver::new();
    let plan_id = "11111111-1111-1111-1111-111111111111";
    let log = StubLog {
        rows: vec![fixture_plan_node_logged_event(
            123,
            plan_id,
            "n1",
            Some(1),
            "ready",
            "running",
        )],
        ..Default::default()
    };
    let r = resolver
        .lookup_or_query_plan_node_state_change(&log, plan_id, "n1", 1, "ready", "running")
        .await;
    assert_eq!(r.status, EventRefStatus::Log);
    // Wave-18 / task 01 — first hit goes through the typed event-log
    // query so provenance must be `EventLogQuery`.
    assert_eq!(
        r.provenance,
        EventRefProvenance::EventLogQuery,
        "first hit (cache miss → typed query) surfaces provenance=event_log_query"
    );
    assert_eq!(r.event_id.as_deref(), Some("123"));
    assert_eq!(r.source.as_deref(), Some("execution"));
    assert_eq!(r.kind.as_deref(), Some("plan_node_state_changed"));
    // Subsequent lookup must NOT re-scan the log — the cache should
    // have been populated by the previous call.
    let calls_before = log.calls.load(std::sync::atomic::Ordering::Relaxed);
    let r2 = resolver
        .lookup_or_query_plan_node_state_change(&log, plan_id, "n1", 1, "ready", "running")
        .await;
    assert_eq!(r2.status, EventRefStatus::Log);
    // Cache hit on the second call surfaces `passive_cache` provenance
    // — distinct from the first call which went through the query.
    assert_eq!(
        r2.provenance,
        EventRefProvenance::PassiveCache,
        "second hit comes from the passive cache populated by the first call"
    );
    assert_eq!(r2.event_id.as_deref(), Some("123"));
    let calls_after = log.calls.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        calls_before, calls_after,
        "cache hit must not trigger a log scan"
    );
}

#[tokio::test]
async fn resolver_live_cache_wins_over_log_query() {
    // Pre-populate the cache with a different seq than what the log
    // would return. The cache must win, and the log must NOT be hit.
    let resolver = EventRefResolver::new();
    let plan_id = "22222222-2222-2222-2222-222222222222";
    resolver.record_plan_node_state_change(
        plan_id,
        "n1",
        1,
        "ready",
        "running",
        "execution",
        "plan_node_state_changed",
        "999",
    );
    let log = StubLog {
        rows: vec![fixture_plan_node_logged_event(
            123,
            plan_id,
            "n1",
            Some(1),
            "ready",
            "running",
        )],
        ..Default::default()
    };
    let r = resolver
        .lookup_or_query_plan_node_state_change(&log, plan_id, "n1", 1, "ready", "running")
        .await;
    assert_eq!(r.status, EventRefStatus::Log);
    assert_eq!(
        r.event_id.as_deref(),
        Some("999"),
        "cache value wins over log query"
    );
    assert_eq!(
        log.calls.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "cache hit must short-circuit the log scan"
    );
}

#[tokio::test]
async fn resolver_log_query_returns_unavailable_on_no_match() {
    let resolver = EventRefResolver::new();
    let log = StubLog {
        rows: vec![fixture_plan_node_logged_event(
            7,
            "other-plan",
            "other-node",
            Some(1),
            "ready",
            "running",
        )],
        ..Default::default()
    };
    let r = resolver
        .lookup_or_query_plan_node_state_change(
            &log,
            "missing-plan",
            "missing-node",
            1,
            "ready",
            "running",
        )
        .await;
    assert_eq!(r.status, EventRefStatus::Unavailable);
    assert_eq!(
        r.unavailable_reason.as_deref(),
        Some(EVENT_REF_LOG_QUERY_MISS_REASON),
        "no match in cache or log surfaces the canonical miss reason"
    );
    assert!(r.event_id.is_none());
}

#[tokio::test]
async fn resolver_log_query_error_degrades_to_unavailable_with_warning() {
    let resolver = EventRefResolver::new();
    let log = StubLog {
        err: Some("db connection refused".to_string()),
        ..Default::default()
    };
    let r = resolver
        .lookup_or_query_plan_node_state_change(&log, "p", "n1", 1, "ready", "running")
        .await;
    assert_eq!(
        r.status,
        EventRefStatus::Unavailable,
        "query error must NEVER fail the caller"
    );
    let reason = r.unavailable_reason.expect("reason set on query error");
    assert!(
        reason.starts_with(EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX),
        "reason must carry the canonical prefix; got: {reason}"
    );
    assert!(
        reason.contains("db connection refused"),
        "reason must include the underlying error; got: {reason}"
    );
}

#[tokio::test]
async fn resolver_log_query_picks_newest_match_when_attempt_repeats() {
    // Two rows for the same correlation tuple — newest wins so the
    // recovered ref points at the latest emit.
    let resolver = EventRefResolver::new();
    let plan_id = "33333333-3333-3333-3333-333333333333";
    let log = StubLog {
        rows: vec![
            fixture_plan_node_logged_event(10, plan_id, "n1", Some(1), "ready", "running"),
            fixture_plan_node_logged_event(20, plan_id, "n1", Some(1), "ready", "running"),
        ],
        ..Default::default()
    };
    let r = resolver
        .lookup_or_query_plan_node_state_change(&log, plan_id, "n1", 1, "ready", "running")
        .await;
    assert_eq!(r.status, EventRefStatus::Log);
    assert_eq!(
        r.event_id.as_deref(),
        Some("20"),
        "newest matching seq wins"
    );
}

#[tokio::test]
async fn resolver_log_query_accepts_row_without_attempt() {
    // Producer that omits `attempt` should still be matched as an
    // any-attempt row (mirrors `attempt: Option<u32>` on the event).
    let resolver = EventRefResolver::new();
    let plan_id = "44444444-4444-4444-4444-444444444444";
    let log = StubLog {
        rows: vec![fixture_plan_node_logged_event(
            42, plan_id, "n1", None, "ready", "running",
        )],
        ..Default::default()
    };
    let r = resolver
        .lookup_or_query_plan_node_state_change(&log, plan_id, "n1", 1, "ready", "running")
        .await;
    assert_eq!(r.status, EventRefStatus::Log);
    assert_eq!(r.event_id.as_deref(), Some("42"));
}

// ── wave-17 / task 06 :: top-level event_ref_* surface ──────────────

#[test]
fn primary_event_ref_surface_emits_status_source_for_live() {
    let r = EventRef::live("execution", "plan_node_state_changed", "42");
    let v = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, kind::DISPATCH)
        .with_primary_event_ref(&r, None)
        .add_execution_event(r)
        .into_json();
    assert_eq!(v["event_ref_status"], "live");
    // Wave-18 / task 01 — `event_ref_source` now carries the resolver
    // provenance vocabulary, not the raw wire source. A live ref skips
    // the resolver entirely so its provenance is `live`.
    assert_eq!(v["event_ref_source"], "live");
    assert!(
        v.get("event_ref_warning").is_none(),
        "live ref with no warning omits the warning key"
    );
}

#[test]
fn primary_event_ref_surface_emits_status_source_for_log_passive_cache() {
    // `EventRef::from_log` is the wave-16 cache-hit constructor; its
    // provenance must surface as `passive_cache` so audit consumers can
    // tell the ref came from the in-memory subscriber cache rather
    // than from the persistent event-log query.
    let r = EventRef::from_log("execution", "plan_node_state_changed", "77");
    let v = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, kind::DISPATCH)
        .with_primary_event_ref(&r, Some("recovered from passive cache".to_string()))
        .add_execution_event(r)
        .into_json();
    assert_eq!(v["event_ref_status"], "log");
    assert_eq!(v["event_ref_source"], "passive_cache");
    assert_eq!(v["event_ref_warning"], "recovered from passive cache");
}

#[test]
fn primary_event_ref_surface_emits_status_source_for_event_log_query() {
    // The wave-18 typed query path stamps `event_log_query` so the
    // provenance vocabulary covers the persistent-log tier distinctly
    // from the in-memory passive cache.
    let r = EventRef::from_event_log_query("execution", "plan_node_state_changed", "99");
    let v = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, kind::DISPATCH)
        .with_primary_event_ref(&r, None)
        .add_execution_event(r)
        .into_json();
    assert_eq!(v["event_ref_status"], "log");
    assert_eq!(v["event_ref_source"], "event_log_query");
}

#[test]
fn primary_event_ref_surface_lifts_unavailable_reason_when_no_explicit_warning() {
    let r = EventRef::unavailable(EVENT_REF_LOG_QUERY_MISS_REASON);
    let v = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, kind::DISPATCH)
        .with_primary_event_ref(&r, None)
        .add_execution_event(r)
        .into_json();
    assert_eq!(v["event_ref_status"], "unavailable");
    // Wave-18 / task 01 — unavailable refs surface `event_ref_source =
    // "unavailable"` so the wire form is non-null and routable. Audit
    // consumers can pivot directly on the provenance string without
    // having to detect the absence of the field.
    assert_eq!(v["event_ref_source"], "unavailable");
    assert_eq!(
        v["event_ref_warning"], EVENT_REF_LOG_QUERY_MISS_REASON,
        "unavailable_reason is lifted as the warning when no explicit warning is set"
    );
}

#[test]
fn resolver_lookup_failure_degrades_to_unavailable_with_reason() {
    // Pinning the no-throw contract from the wave-16/07 brief: a
    // missing key MUST return `EventRef::unavailable(...)` rather than
    // panic / Err — the resolver never fails the caller.
    let resolver = EventRefResolver::new();
    let r = resolver.lookup_plan_node_state_change("plan", "node", 1, "ready", "succeeded");
    assert_eq!(r.status, EventRefStatus::Unavailable);
    let json = EvidenceEntry::new("test", kind::DISPATCH)
        .add_execution_event(r)
        .into_json();
    let arr = json["execution_events"].as_array().unwrap();
    assert_eq!(arr[0]["status"], "unavailable");
    assert_eq!(arr[0]["unavailable"], true);
    assert_eq!(arr[0]["unavailable_reason"], EVENT_REF_RESOLVER_MISS_REASON);
}

// ── extra / merge passthrough ─────────────────────────────────────

#[test]
fn extra_passthrough_preserves_legacy_keys() {
    let entry = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, kind::DISPATCH)
        .with_extra("node_id", json!("n1"))
        .with_extra("target_tool", json!("mission_execution"))
        .with_extra("dispatch_strategy", json!("agent-team"));
    let v = entry.into_json();
    assert_eq!(v["node_id"], "n1");
    assert_eq!(v["target_tool"], "mission_execution");
    assert_eq!(v["dispatch_strategy"], "agent-team");
}

#[test]
fn merge_extra_skips_canonical_stamps() {
    // Caller hands in a legacy-shaped object including `schema_version`
    // and `source` — those must NOT clobber what the builder set.
    let legacy = json!({
        "schema_version": "deadbeef",
        "source": "some_other_source",
        "kind": "some_other_kind",
        "recorded_at": "yesterday",
        "node_id": "n7",
    });
    let entry =
        EvidenceEntry::new(source::PLAN_RUNNER_DISPATCH, kind::DISPATCH).merge_extra(legacy);
    let v = entry.into_json();
    // Canonical stamps preserved.
    assert_eq!(v["schema_version"], EVIDENCE_SCHEMA_VERSION);
    assert_eq!(v["source"], source::PLAN_RUNNER_DISPATCH);
    assert_eq!(v["kind"], kind::DISPATCH);
    assert!(
        v.get("recorded_at").is_none(),
        "recorded_at stamped by writer, not builder"
    );
    // Non-canonical legacy keys come through.
    assert_eq!(v["node_id"], "n7");
}

#[test]
fn typed_inner_dispatch_wins_over_extra_legacy_inner_dispatch() {
    // If a caller both sets `with_inner_dispatch` AND merges legacy JSON
    // that also has `inner_dispatch`, the typed one is authoritative.
    let entry = EvidenceEntry::new(source::PLAN_RUNNER_DISPATCH, kind::DISPATCH)
        .with_inner_dispatch(json!({"typed": true}))
        .merge_extra(json!({"inner_dispatch": {"typed": false}}));
    let v = entry.into_json();
    assert_eq!(v["inner_dispatch"]["typed"], true);
}

// ── legacy record_evidence wrapping ──────────────────────────────

#[test]
fn wrap_legacy_keeps_inner_evidence_under_evidence_key() {
    let inner = json!({"tool_calls": [{"name": "cargo test", "exit": 0}]});
    let v = wrap_legacy_record_evidence(inner.clone(), None, None);
    let obj = v.as_object().unwrap();
    assert_eq!(obj["schema_version"], EVIDENCE_SCHEMA_VERSION);
    assert_eq!(obj["source"], source::RECORD_EVIDENCE_MANUAL);
    assert_eq!(obj["kind"], kind::NOTE);
    assert_eq!(obj["evidence"], inner);
}

#[test]
fn wrap_legacy_honours_kind_and_source_overrides() {
    let inner = json!({"note": "build green"});
    let v = wrap_legacy_record_evidence(inner, Some(kind::VERIFICATION), Some("custom-source"));
    assert_eq!(v["kind"], kind::VERIFICATION);
    assert_eq!(v["source"], "custom-source");
}

// ── AppendOutcome conversion ──────────────────────────────────────

#[test]
fn append_outcome_legacy_tuple_preserves_path() {
    let outcome = AppendOutcome::Written {
        path: PathBuf::from("/tmp/x.json"),
        entry_count: 3,
    };
    let (path, err) = outcome.into_legacy_tuple();
    assert_eq!(path.as_deref(), Some("/tmp/x.json"));
    assert!(err.is_none());
}

#[test]
fn append_outcome_legacy_tuple_preserves_error() {
    let outcome = AppendOutcome::Failed {
        error: "write failed".to_string(),
    };
    let (path, err) = outcome.into_legacy_tuple();
    assert!(path.is_none());
    assert_eq!(err.as_deref(), Some("write failed"));
}

// ── sidecar append integration (multi-entry order + schema_version) ─

/// End-to-end: stage two entries through the lower-level
/// [`append_entry_to_project_root`] writer and verify the on-disk sidecar
/// contains both, in order, each with `schema_version` stamped. We use
/// the lower-level writer (instead of the public `append(...)` which
/// requires a full `AppState`) because it exercises the same on-disk
/// shape contract — `append_plan_evidence_entry` and
/// `append_entry_to_project_root` are kept in lockstep so a reader cannot
/// tell which one wrote a given sidecar.
#[test]
fn sidecar_append_preserves_order_and_schema_version() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();

    let plan_id = uuid::Uuid::new_v4();

    // Entry #1: typed verification entry
    let e1 = EvidenceEntry::new(source::PLAN_RUNNER_DISPATCH, kind::VERIFICATION)
        .with_verification(vec!["cargo test".to_string()], json!({"passed": true}))
        .with_extra("seq", json!(1));
    let (path1, count1) = append_entry_to_project_root(&root, plan_id, e1.into_json())
        .expect("first append succeeds");
    assert_eq!(count1, 1, "first append → single entry");

    // Entry #2: typed dispatch entry
    let e2 = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, kind::DISPATCH)
        .with_inner_dispatch(json!({"status": "executing"}))
        .with_state_transition("ready -> succeeded")
        .with_extra("seq", json!(2));
    let (path2, count2) = append_entry_to_project_root(&root, plan_id, e2.into_json())
        .expect("second append succeeds");
    assert_eq!(count2, 2, "second append → two entries total");
    assert_eq!(path1, path2, "both entries land in the same sidecar file");

    // Read back and assert the order / schema_version are correct.
    let raw = std::fs::read_to_string(&path2).expect("sidecar exists");
    let bundle: Value = serde_json::from_str(&raw).expect("valid json");
    let entries = bundle["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2, "both entries persisted");

    // Order preserved: seq 1 then 2.
    assert_eq!(entries[0]["seq"], 1);
    assert_eq!(entries[1]["seq"], 2);

    // Schema_version stamped on every entry.
    assert_eq!(entries[0]["schema_version"], EVIDENCE_SCHEMA_VERSION);
    assert_eq!(entries[1]["schema_version"], EVIDENCE_SCHEMA_VERSION);

    // Source / kind round-trip.
    assert_eq!(entries[0]["source"], source::PLAN_RUNNER_DISPATCH);
    assert_eq!(entries[0]["kind"], kind::VERIFICATION);
    assert_eq!(entries[1]["source"], source::PLAN_DAG_NODE_DISPATCH);
    assert_eq!(entries[1]["kind"], kind::DISPATCH);

    // recorded_at stamped by the underlying writer (not the builder).
    assert!(
        entries[0].get("recorded_at").is_some(),
        "writer stamps recorded_at"
    );
    assert!(entries[1].get("recorded_at").is_some());
}

/// Failure path: writing into a non-existent directory whose parent we
/// cannot create must surface a structured error, not silently succeed.
/// We point the writer at a path under a regular file (which can never
/// host a `.missiond/v2/plans/` subtree) so `mkdir` reliably fails.
#[test]
fn sidecar_append_surfaces_writer_failure() {
    let tmp = tempfile::tempdir().unwrap();
    // Create a file, then ask the writer to put `.missiond/v2/plans/` under it.
    let blocker = tmp.path().join("not_a_dir");
    std::fs::write(&blocker, b"i am a file").unwrap();
    let plan_id = uuid::Uuid::new_v4();
    let e = EvidenceEntry::new(source::RECORD_EVIDENCE_MANUAL, kind::NOTE).into_json();
    let err = append_entry_to_project_root(&blocker, plan_id, e)
        .expect_err("writer must reject path under a regular file");
    // mkdir fails because `blocker` is a file, not a directory; the
    // exact error message depends on the platform, but it must mention
    // mkdir so callers can route on it.
    assert!(
        err.to_string().contains("mkdir"),
        "error must explain mkdir failure, got: {}",
        err
    );
}

/// Append-only invariant: when the sidecar already exists with N
/// entries, a subsequent append produces N+1 entries — the writer must
/// never overwrite the bundle.
#[test]
fn sidecar_append_is_strictly_additive() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let plan_id = uuid::Uuid::new_v4();

    for i in 0..5 {
        let e = EvidenceEntry::new(source::RECORD_EVIDENCE_MANUAL, kind::NOTE)
            .with_extra("seq", json!(i));
        let (_p, count) =
            append_entry_to_project_root(&root, plan_id, e.into_json()).expect("append succeeds");
        assert_eq!(count as i64, i + 1, "append #{i} → {} entries", i + 1);
    }
}

// ── commit metadata preservation ─────────────────────────────────

/// Commit handoff metadata supplied via the typed setter must round-trip
/// through into_json without mutation. This covers the wave-12 task-01
/// `record scoped commit handoff metadata` integration: callers stamp
/// commit_hash + commit_status, the collector preserves them verbatim
/// under the canonical `commit` key.
#[test]
fn commit_metadata_round_trip_via_typed_setter() {
    let entry = EvidenceEntry::new(source::PLAN_RUNNER_DISPATCH, kind::COMMIT).with_commit(
        Some("a1b2c3d4e5f6".to_string()),
        Some("scoped_commit".to_string()),
    );
    let v = entry.into_json();
    assert_eq!(v["commit"]["commit_hash"], "a1b2c3d4e5f6");
    assert_eq!(v["commit"]["commit_status"], "scoped_commit");
}

/// Commit handoff metadata supplied via the legacy `merge_extra` path
/// (caller already had a JSON object with `commit_hash` / `commit_status`
/// at the top level — pre-collector wire form) must also reach the
/// sidecar without loss. Rules out a refactor regression where the typed
/// `commit` key gets serialised but a legacy producer's flat keys get
/// dropped on the floor.
#[test]
fn commit_metadata_round_trip_via_legacy_merge() {
    let legacy_payload = json!({
        "commit_hash": "deadbeef",
        "commit_status": "scoped_commit",
        "scope_path": "crates/missiond-daemon/src/handlers/knowledge",
    });
    let entry = EvidenceEntry::new(source::PLAN_RUNNER_DISPATCH, kind::DISPATCH)
        .merge_extra(legacy_payload);
    let v = entry.into_json();
    // Legacy flat keys preserved.
    assert_eq!(v["commit_hash"], "deadbeef");
    assert_eq!(v["commit_status"], "scoped_commit");
    assert_eq!(
        v["scope_path"],
        "crates/missiond-daemon/src/handlers/knowledge"
    );
}

// ── wave-18 / task 05 — cross-plan distill chain v0 ──────────────────
//
// The chain orchestrator (super::plan::apply_distill_chain) appends
// exactly ONE chain-record entry per plan finalization. Multiple
// plans pinning the same chain_id form a chain — each lands in its
// own sidecar; the writer is purely additive so prior chain entries
// (in this OR other plans' sidecars) are NEVER overwritten.
//
// These tests pin the on-disk shape contract from the
// evidence_collector side (the writer half) so the chain
// orchestrator's "append never overwrites" invariant is testable
// without standing up a full daemon.

/// Two appends with the same chain_id under the same plan id MUST
/// land as TWO entries in the bundle — the second never overwrites
/// the first. Exercises the wave-18 / task 05 brief invariant
/// `append new plan result; do not overwrite prior evidence`.
#[test]
fn distill_chain_records_are_strictly_additive_per_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let plan_id = uuid::Uuid::new_v4();

    // Entry #1: chain-record from a first finalize pass.
    let e1 = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, "distill_chain_record")
        .with_state_transition("distill_chain_appended")
        .with_extra("chain_id", json!("chain:wave18-loop"))
        .with_extra("chain_index_in_plan", json!(1))
        .with_extra("chain_mode", json!("record_only"))
        .with_extra("plan_id", json!(plan_id));
    let (path, count1) = append_entry_to_project_root(&root, plan_id, e1.into_json())
        .expect("first chain append succeeds");
    assert_eq!(count1, 1);

    // Entry #2: a SECOND chain-record append against the SAME plan
    // (e.g. caller re-ran action_execute against an already-
    // finalized plan with the same chain id). Must NOT clobber the
    // first row.
    let e2 = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, "distill_chain_record")
        .with_state_transition("distill_chain_appended")
        .with_extra("chain_id", json!("chain:wave18-loop"))
        .with_extra("chain_index_in_plan", json!(2))
        .with_extra("chain_mode", json!("record_only"))
        .with_extra("plan_id", json!(plan_id));
    let (path2, count2) = append_entry_to_project_root(&root, plan_id, e2.into_json())
        .expect("second chain append succeeds");
    assert_eq!(count2, 2, "second chain append → two entries total");
    assert_eq!(path, path2, "both chain entries land in the same sidecar");

    // Read back: BOTH chain entries persisted, in append order.
    let raw = std::fs::read_to_string(&path2).expect("sidecar exists");
    let bundle: Value = serde_json::from_str(&raw).expect("valid json");
    let entries = bundle["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2, "both chain entries persisted");
    // Same chain id on both — they belong to the same chain bucket.
    assert_eq!(entries[0]["chain_id"], "chain:wave18-loop");
    assert_eq!(entries[1]["chain_id"], "chain:wave18-loop");
    // Index field round-trips so the chain orchestrator can rebuild
    // chronological order from a single sidecar read.
    assert_eq!(entries[0]["chain_index_in_plan"], 1);
    assert_eq!(entries[1]["chain_index_in_plan"], 2);
    // Schema_version stamped on every entry so downstream consumers
    // can route on it explicitly when the chain shape evolves.
    assert_eq!(entries[0]["schema_version"], EVIDENCE_SCHEMA_VERSION);
    assert_eq!(entries[1]["schema_version"], EVIDENCE_SCHEMA_VERSION);
}

/// A chain entry appended to a sidecar that ALREADY carries non-
/// chain entries (e.g. wave-17 dag_finalized rows, per-node
/// dispatch rows) must coexist with them — the wave-18 orchestrator
/// only adds rows, never rewrites the bundle. This pins the
/// invariant "do not overwrite prior evidence" from the brief at
/// the most aggressive interpretation: prior evidence of ANY kind.
#[test]
fn distill_chain_record_coexists_with_prior_non_chain_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let plan_id = uuid::Uuid::new_v4();

    // Pre-existing wave-17 dag_finalized row.
    let prior = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, kind::NOTE)
        .with_state_transition("dag_finalized")
        .with_extra("aggregate_status", json!("dag_succeeded"))
        .with_extra("final_plan_status", json!("succeeded"));
    let (_p1, c1) = append_entry_to_project_root(&root, plan_id, prior.into_json())
        .expect("prior dag_finalized append succeeds");
    assert_eq!(c1, 1);

    // Wave-18 chain entry appended after the wave-17 finalize row.
    let chain = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, "distill_chain_record")
        .with_state_transition("distill_chain_appended")
        .with_extra("chain_id", json!("chain:wave18"))
        .with_extra("chain_mode", json!("record_only"));
    let (path, c2) = append_entry_to_project_root(&root, plan_id, chain.into_json())
        .expect("chain append succeeds");
    assert_eq!(c2, 2);

    // Read back: prior wave-17 row preserved, chain row appended.
    let raw = std::fs::read_to_string(&path).expect("sidecar exists");
    let bundle: Value = serde_json::from_str(&raw).expect("valid json");
    let entries = bundle["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["state_transition"], "dag_finalized");
    assert_eq!(entries[0]["aggregate_status"], "dag_succeeded");
    assert_eq!(entries[1]["state_transition"], "distill_chain_appended");
    assert_eq!(entries[1]["chain_id"], "chain:wave18");
    // Source tag round-trips on both rows so audit dashboards can
    // pivot on `source` without re-deriving from the kind.
    assert_eq!(entries[0]["source"], source::PLAN_DAG_NODE_DISPATCH);
    assert_eq!(entries[1]["source"], source::PLAN_DAG_NODE_DISPATCH);
}

/// Two different chain ids under the same plan_id must coexist
/// without one overwriting the other. Exercises the case where a
/// caller participates in multiple chains from the same plan (e.g.
/// one chain for "release distillation", another for "weekly
/// rollup") — the writer treats each entry independently.
#[test]
fn distill_chain_records_from_different_chains_coexist() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let plan_id = uuid::Uuid::new_v4();

    for (idx, chain_id) in ["chain:release-rc1", "chain:weekly-rollup"]
        .iter()
        .enumerate()
    {
        let e = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, "distill_chain_record")
            .with_state_transition("distill_chain_appended")
            .with_extra("chain_id", json!(*chain_id))
            .with_extra("chain_index_in_plan", json!(1));
        let (_p, count) = append_entry_to_project_root(&root, plan_id, e.into_json())
            .expect("chain append succeeds");
        assert_eq!(count, idx + 1);
    }

    let path = root
        .join(".missiond/v2/plans")
        .join(format!("{}.evidence.json", plan_id));
    let raw = std::fs::read_to_string(&path).expect("sidecar exists");
    let bundle: Value = serde_json::from_str(&raw).expect("valid json");
    let entries = bundle["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["chain_id"], "chain:release-rc1");
    assert_eq!(entries[1]["chain_id"], "chain:weekly-rollup");
}
