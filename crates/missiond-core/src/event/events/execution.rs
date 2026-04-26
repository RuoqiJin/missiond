//! `ExecutionEvent` — live projection of `mission_execution`'s
//! agent-execution-coordination companion log.
//!
//! Lisp authority:
//!   - intent-event-bus.lisp :: planned-event-extensions :: ExecutionEvent
//!     (:candidate-domain "Execution")
//!   - intent-flow.lisp :: F-execution-log-governance
//!   - intent-memory.lisp :: helper agent-execution-coordination v0.5.x
//!
//! Durable evidence remains the on-disk `<project_root>/.missiond/v2/<id>.lisp`
//! companion file. The bus event is a non-authoritative live notification —
//! consumers (status dashboards, notification surfaces, audit projections)
//! react to it but the file is truth. See `:rationale` in
//! `planned-event-extensions :: ExecutionEvent`.
//!
//! All ID fields (`claim_id`, `deviation_id`, `decision_id`, `issue_id`,
//! `completion_id`) carry the helper protocol's prefix-encoded form
//! (`C001`, `D003`, `DC002`, `I004`, `COMP005`) so consumers can correlate
//! back to the companion log without re-deriving the format.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionEvent {
    /// New companion log opened by `mission_execution(action=open)`.
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log
    /// (intent-worker.lisp :: claudecode-workstation-orchestration ::
    /// execution-strategy-record). They are optional + skipped on serialize
    /// when absent so legacy producers / consumers stay byte-identical with
    /// the original 5-field wire form. Durable truth still lives in the
    /// on-disk `<project_root>/.missiond/v2/<id>.lisp` companion file; the
    /// event metadata is a live projection for status / audit consumers.
    Opened {
        execution_id: String,
        parent_design: String,
        scope: String,
        owner: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// A new claim was acquired over a scope.
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block (intent-worker.lisp :: claudecode-workstation-orchestration ::
    /// execution-strategy-record). They are read from the on-disk meta on
    /// each emit so the consumer does not have to re-load the companion
    /// log to correlate this claim against its dispatch context. Optional
    /// + skipped on serialize when absent so legacy producers / consumers
    /// stay byte-identical with the original 6-field wire form.
    Claimed {
        execution_id: String,
        claim_id: String,
        claimer: String,
        scope: String,
        phase: String,
        lease_expires_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// Heartbeat extended an active claim's lease.
    Heartbeat {
        execution_id: String,
        claim_id: String,
        claimer: String,
        heartbeat_at: String,
        lease_expires_at: String,
    },
    /// Claim was released by its owner (clean handoff).
    Released {
        execution_id: String,
        claim_id: String,
        claimer: String,
        released_at: String,
        summary: Option<String>,
    },
    /// A deviation between Lisp design and observed reality was recorded.
    DeviationRecorded {
        execution_id: String,
        deviation_id: String,
        phase: String,
        approved_by: String,
    },
    /// A design decision was recorded against the execution.
    DecisionRecorded {
        execution_id: String,
        decision_id: String,
        decided_by: String,
        at: String,
    },
    /// A blocking/non-blocking issue was raised against the execution.
    IssueRecorded {
        execution_id: String,
        issue_id: String,
        severity: String,
        owner: String,
    },
    /// A phase or subtask was marked complete.
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block (intent-worker.lisp :: claudecode-workstation-orchestration ::
    /// execution-strategy-record). They are read from the on-disk meta on
    /// each emit so the consumer does not have to re-load the companion
    /// log to correlate this completion against its dispatch context.
    /// Optional + skipped on serialize when absent so legacy producers /
    /// consumers stay byte-identical with the original 5-field wire form.
    Completed {
        execution_id: String,
        completion_id: String,
        phase: String,
        agent: String,
        at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// An audit run finished — `findings_count` is the total findings of any
    /// severity, `error_count` the subset that block (parse / overlap / dup).
    Audited {
        execution_id: String,
        ok: bool,
        findings_count: u32,
        error_count: u32,
    },
    /// A repair pass ran — `applied=false` means dry-run preview only.
    Repaired {
        execution_id: String,
        applied: bool,
        action_count: u32,
    },
    /// The audit / repair pass detected an active claim whose lease has
    /// expired without release. Emitted standalone so notifiers can react
    /// without scanning the full audit findings.
    StaleClaim {
        execution_id: String,
        claim_id: String,
        claimer: String,
        lease_expires_at: String,
    },
    /// Per-node lifecycle transition observed by the PLAN DAG runtime v2
    /// (`plan_dag.rs`). Bus is non-authoritative — durable evidence still
    /// lives in `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json`
    /// (see `evidence_collector` :: `plan_dag_node_dispatch` entries) — but
    /// dashboards / replayers can correlate on `plan_id + node_id + attempt`
    /// without scraping the sidecar file. `from`/`to` mirror the scheduler
    /// `NodeLifecycle` discriminants
    /// (`pending` | `ready` | `running` | `succeeded` | `failed` | `skipped`).
    /// `target` is the inner-dispatch tool (`mission_execution` /
    /// `mission_task_delegate` / `mission_flow_run`); `dispatch_strategy` /
    /// `target_project` mirror the same fields surfaced on `Opened` so a
    /// downstream consumer can join the two streams without re-deriving the
    /// metadata. `attempt` defaults to 1 (v2 has no per-node retry); a future
    /// retry-aware scheduler bumps it without requiring a new variant.
    /// `reason` carries the skip / failure annotation
    /// (`upstream_failed` / `condition_gated` / `fail_fast_aborted` /
    /// inner-error message); omitted for plain success transitions.
    ///
    /// All non-id fields are optional + skip-serialize so a producer that
    /// only knows `plan_id`/`node_id`/`from`/`to` stays wire-compatible with
    /// future shape extensions.
    PlanNodeStateChanged {
        plan_id: String,
        node_id: String,
        from: String,
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attempt: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl DomainEvent for ExecutionEvent {
    fn domain() -> Domain {
        Domain::Execution
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Opened { .. } => "opened",
            Self::Claimed { .. } => "claimed",
            Self::Heartbeat { .. } => "heartbeat",
            Self::Released { .. } => "released",
            Self::DeviationRecorded { .. } => "deviation_recorded",
            Self::DecisionRecorded { .. } => "decision_recorded",
            Self::IssueRecorded { .. } => "issue_recorded",
            Self::Completed { .. } => "completed",
            Self::Audited { .. } => "audited",
            Self::Repaired { .. } => "repaired",
            Self::StaleClaim { .. } => "stale_claim",
            Self::PlanNodeStateChanged { .. } => "plan_node_state_changed",
        }
    }

    fn payload_size_hint(&self) -> usize {
        // All variants are small id+label tuples; default 256 covers them.
        256
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_execution() {
        assert_eq!(ExecutionEvent::domain(), Domain::Execution);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            ExecutionEvent::Opened {
                execution_id: "e".into(),
                parent_design: "p".into(),
                scope: "s".into(),
                owner: "o".into(),
                path: "/tmp/e.lisp".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::Claimed {
                execution_id: "e".into(),
                claim_id: "C001".into(),
                claimer: "a".into(),
                scope: "s".into(),
                phase: "".into(),
                lease_expires_at: "2026-04-25T00:00:00Z".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::Heartbeat {
                execution_id: "e".into(),
                claim_id: "C001".into(),
                claimer: "a".into(),
                heartbeat_at: "2026-04-25T00:00:00Z".into(),
                lease_expires_at: "2026-04-25T00:30:00Z".into(),
            },
            ExecutionEvent::Released {
                execution_id: "e".into(),
                claim_id: "C001".into(),
                claimer: "a".into(),
                released_at: "2026-04-25T00:00:00Z".into(),
                summary: None,
            },
            ExecutionEvent::DeviationRecorded {
                execution_id: "e".into(),
                deviation_id: "D001".into(),
                phase: "p".into(),
                approved_by: "auto".into(),
            },
            ExecutionEvent::DecisionRecorded {
                execution_id: "e".into(),
                decision_id: "DC001".into(),
                decided_by: "a".into(),
                at: "2026-04-25T00:00:00Z".into(),
            },
            ExecutionEvent::IssueRecorded {
                execution_id: "e".into(),
                issue_id: "I001".into(),
                severity: "medium".into(),
                owner: "".into(),
            },
            ExecutionEvent::Completed {
                execution_id: "e".into(),
                completion_id: "COMP001".into(),
                phase: "p".into(),
                agent: "a".into(),
                at: "2026-04-25T00:00:00Z".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::Audited {
                execution_id: "e".into(),
                ok: true,
                findings_count: 0,
                error_count: 0,
            },
            ExecutionEvent::Repaired {
                execution_id: "e".into(),
                applied: false,
                action_count: 0,
            },
            ExecutionEvent::StaleClaim {
                execution_id: "e".into(),
                claim_id: "C001".into(),
                claimer: "a".into(),
                lease_expires_at: "2026-04-25T00:00:00Z".into(),
            },
            ExecutionEvent::PlanNodeStateChanged {
                plan_id: "00000000-0000-0000-0000-000000000abc".into(),
                node_id: "n1".into(),
                from: "ready".into(),
                to: "running".into(),
                target: Some("mission_execution".into()),
                dispatch_strategy: Some("agent-team".into()),
                target_project: Some("missiond".into()),
                attempt: Some(1),
                reason: None,
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip_for_each_variant() {
        let cases = [
            ExecutionEvent::Opened {
                execution_id: "exec-1".into(),
                parent_design: "intent-event-bus.lisp".into(),
                scope: "src/event/**".into(),
                owner: "claude".into(),
                path: ".missiond/v2/exec-1.lisp".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::Opened {
                execution_id: "exec-2".into(),
                parent_design: "intent-worker.lisp".into(),
                scope: "src/handlers/**".into(),
                owner: "claude".into(),
                path: ".missiond/v2/exec-2.lisp".into(),
                dispatch_strategy: Some("fresh-code-alignment".into()),
                target_project: Some("missiond".into()),
                requested_cwd: Some("/Users/x/Projects/missiond".into()),
            },
            ExecutionEvent::Claimed {
                execution_id: "exec-1".into(),
                claim_id: "C001".into(),
                claimer: "claude".into(),
                scope: "src/event/events/".into(),
                phase: "phase-A".into(),
                lease_expires_at: "2026-04-25T01:00:00Z".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::Released {
                execution_id: "exec-1".into(),
                claim_id: "C001".into(),
                claimer: "claude".into(),
                released_at: "2026-04-25T00:50:00Z".into(),
                summary: Some("done".into()),
            },
            ExecutionEvent::Audited {
                execution_id: "exec-1".into(),
                ok: false,
                findings_count: 3,
                error_count: 1,
            },
        ];
        for ev in &cases {
            let json = serde_json::to_string(ev).unwrap();
            let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(ev, &back);
        }
    }

    #[test]
    fn payload_hint_is_small_for_all_variants() {
        let ev = ExecutionEvent::StaleClaim {
            execution_id: "e".into(),
            claim_id: "C001".into(),
            claimer: "a".into(),
            lease_expires_at: "2026-04-25T00:00:00Z".into(),
        };
        assert!(ev.payload_size_hint() <= 1024);
    }

    /// Legacy producers wrote `Opened` with only the original 5 fields.
    /// `#[serde(default)]` on the new `Option<String>` slots must let the
    /// old shape deserialize into `None` everywhere without erroring.
    #[test]
    fn opened_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "Opened": {
                "execution_id": "exec-legacy",
                "parent_design": "old.lisp",
                "scope": "old/scope",
                "owner": "old-owner",
                "path": ".missiond/v2/exec-legacy.lisp"
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::Opened {
                ref execution_id,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
                ..
            } => {
                assert_eq!(execution_id, "exec-legacy");
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected Opened"),
        }
    }

    /// When all dispatch metadata is absent, the serialized JSON must be
    /// byte-identical to the old 5-field wire form (no extra keys for
    /// `dispatch_strategy` / `target_project` / `requested_cwd`).
    /// `skip_serializing_if = "Option::is_none"` enforces this.
    #[test]
    fn opened_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::Opened {
            execution_id: "exec-1".into(),
            parent_design: "p.lisp".into(),
            scope: "s".into(),
            owner: "o".into(),
            path: "/tmp/e.lisp".into(),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let opened = parsed
            .get("Opened")
            .and_then(|v| v.as_object())
            .expect("Opened payload");
        assert!(!opened.contains_key("dispatch_strategy"));
        assert!(!opened.contains_key("target_project"));
        assert!(!opened.contains_key("requested_cwd"));
        for key in ["execution_id", "parent_design", "scope", "owner", "path"] {
            assert!(opened.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(opened.len(), 5);
    }

    /// New producers may surface dispatch metadata; round-trip must preserve
    /// each provided value verbatim and re-serialize exactly the keys
    /// present.
    #[test]
    fn opened_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::Opened {
            execution_id: "exec-disp".into(),
            parent_design: "p.lisp".into(),
            scope: "s".into(),
            owner: "claude".into(),
            path: ".missiond/v2/exec-disp.lisp".into(),
            dispatch_strategy: Some("agent-team".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond/crates".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let opened = parsed
            .get("Opened")
            .and_then(|v| v.as_object())
            .expect("Opened payload");
        assert_eq!(opened.len(), 8);
        assert_eq!(
            opened.get("dispatch_strategy").and_then(|v| v.as_str()),
            Some("agent-team")
        );
        assert_eq!(
            opened.get("target_project").and_then(|v| v.as_str()),
            Some("missiond")
        );
        assert_eq!(
            opened.get("requested_cwd").and_then(|v| v.as_str()),
            Some("/Users/x/Projects/missiond/crates")
        );
    }

    // ── PlanNodeStateChanged ──────────────────────────────────────────

    /// Round-trip the variant with every optional slot populated. Pins the
    /// wire shape the PLAN DAG runtime v2 emits: 4 required ids plus 5
    /// optional metadata slots.
    #[test]
    fn plan_node_state_changed_round_trip_full() {
        let ev = ExecutionEvent::PlanNodeStateChanged {
            plan_id: "00000000-0000-0000-0000-000000000abc".into(),
            node_id: "n1".into(),
            from: "running".into(),
            to: "succeeded".into(),
            target: Some("mission_execution".into()),
            dispatch_strategy: Some("agent-team".into()),
            target_project: Some("missiond".into()),
            attempt: Some(1),
            reason: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("PlanNodeStateChanged")
            .and_then(|v| v.as_object())
            .expect("PlanNodeStateChanged payload");
        // 4 required + 4 supplied optional = 8 keys (reason is None → skipped).
        assert_eq!(payload.len(), 8, "reason omitted when None");
        assert!(!payload.contains_key("reason"));
        assert_eq!(payload["plan_id"], "00000000-0000-0000-0000-000000000abc");
        assert_eq!(payload["from"], "running");
        assert_eq!(payload["to"], "succeeded");
        assert_eq!(payload["target"], "mission_execution");
        assert_eq!(payload["dispatch_strategy"], "agent-team");
        assert_eq!(payload["target_project"], "missiond");
        assert_eq!(payload["attempt"], 1);
    }

    /// Minimal-shape producer (only the four required id fields). Every
    /// optional must skip-serialize so the wire form is exactly 4 keys.
    #[test]
    fn plan_node_state_changed_minimal_serializes_only_required_keys() {
        let ev = ExecutionEvent::PlanNodeStateChanged {
            plan_id: "p".into(),
            node_id: "n".into(),
            from: "pending".into(),
            to: "skipped".into(),
            target: None,
            dispatch_strategy: None,
            target_project: None,
            attempt: None,
            reason: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("PlanNodeStateChanged")
            .and_then(|v| v.as_object())
            .expect("PlanNodeStateChanged payload");
        assert_eq!(payload.len(), 4, "only the 4 required id fields land");
        for key in ["plan_id", "node_id", "from", "to"] {
            assert!(payload.contains_key(key), "missing required key {}", key);
        }
        for key in ["target", "dispatch_strategy", "target_project", "attempt", "reason"] {
            assert!(!payload.contains_key(key), "absent optional {} must skip-serialize", key);
        }
    }

    /// Failure / skip transitions surface a `reason` annotation. Bus
    /// consumers route on this without re-fetching the evidence sidecar.
    #[test]
    fn plan_node_state_changed_failure_reason_round_trips() {
        let ev = ExecutionEvent::PlanNodeStateChanged {
            plan_id: "p".into(),
            node_id: "n2".into(),
            from: "pending".into(),
            to: "skipped".into(),
            target: Some("mission_execution".into()),
            dispatch_strategy: Some("unknown".into()),
            target_project: None,
            attempt: Some(1),
            reason: Some("upstream_failed:n1".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("PlanNodeStateChanged")
            .and_then(|v| v.as_object())
            .expect("PlanNodeStateChanged payload");
        assert_eq!(payload["reason"], "upstream_failed:n1");
    }

    /// `kind()` returns the canonical wire tag — the evidence collector
    /// composes `EventRef::new("execution", kind, event_id)` from this exact
    /// string.
    #[test]
    fn plan_node_state_changed_kind_is_canonical() {
        let ev = ExecutionEvent::PlanNodeStateChanged {
            plan_id: "p".into(),
            node_id: "n".into(),
            from: "pending".into(),
            to: "running".into(),
            target: None,
            dispatch_strategy: None,
            target_project: None,
            attempt: None,
            reason: None,
        };
        assert_eq!(ev.kind(), "plan_node_state_changed");
    }

    // ── Claimed / Completed dispatch metadata ────────────────────────
    //
    // Wave 18 / Task 02 extended `Claimed` and `Completed` with the same
    // optional dispatch metadata trio carried by `Opened` so consumers
    // observing claim / completion events can correlate against the
    // workstation-dispatch context without re-loading the companion log.
    // Backward compatibility is mandatory: legacy 6-field `Claimed` /
    // 5-field `Completed` JSON must still deserialize, and producers that
    // omit the trio must serialize byte-identically to the legacy wire
    // form.

    /// Legacy producers wrote `Claimed` with only the original 6 fields.
    /// `#[serde(default)]` on the new optionals must let the old shape
    /// deserialize into `None` everywhere without erroring.
    #[test]
    fn claimed_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "Claimed": {
                "execution_id": "exec-legacy",
                "claim_id": "C001",
                "claimer": "old-claimer",
                "scope": "old/scope",
                "phase": "phase-A",
                "lease_expires_at": "2026-04-25T01:00:00Z"
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::Claimed {
                ref execution_id,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
                ..
            } => {
                assert_eq!(execution_id, "exec-legacy");
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected Claimed"),
        }
    }

    /// When all dispatch metadata is absent, `Claimed` must serialize
    /// byte-identical to the old 6-field wire form (no extra keys for
    /// the dispatch trio).
    #[test]
    fn claimed_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::Claimed {
            execution_id: "exec-1".into(),
            claim_id: "C001".into(),
            claimer: "claude".into(),
            scope: "src/event/events/".into(),
            phase: "phase-A".into(),
            lease_expires_at: "2026-04-25T01:00:00Z".into(),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let claimed = parsed
            .get("Claimed")
            .and_then(|v| v.as_object())
            .expect("Claimed payload");
        assert!(!claimed.contains_key("dispatch_strategy"));
        assert!(!claimed.contains_key("target_project"));
        assert!(!claimed.contains_key("requested_cwd"));
        for key in [
            "execution_id",
            "claim_id",
            "claimer",
            "scope",
            "phase",
            "lease_expires_at",
        ] {
            assert!(claimed.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(claimed.len(), 6);
    }

    /// New producers may surface dispatch metadata on `Claimed`; the
    /// round-trip must preserve every provided value verbatim.
    #[test]
    fn claimed_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::Claimed {
            execution_id: "exec-disp".into(),
            claim_id: "C002".into(),
            claimer: "claude".into(),
            scope: "src/event/events/".into(),
            phase: "phase-B".into(),
            lease_expires_at: "2026-04-25T02:00:00Z".into(),
            dispatch_strategy: Some("agent-team".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond/crates".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let claimed = parsed
            .get("Claimed")
            .and_then(|v| v.as_object())
            .expect("Claimed payload");
        assert_eq!(claimed.len(), 9);
        assert_eq!(
            claimed.get("dispatch_strategy").and_then(|v| v.as_str()),
            Some("agent-team")
        );
        assert_eq!(
            claimed.get("target_project").and_then(|v| v.as_str()),
            Some("missiond")
        );
        assert_eq!(
            claimed.get("requested_cwd").and_then(|v| v.as_str()),
            Some("/Users/x/Projects/missiond/crates")
        );
    }

    /// Legacy producers wrote `Completed` with only the original 5
    /// fields. `#[serde(default)]` on the new optionals must let the old
    /// shape deserialize into `None` everywhere.
    #[test]
    fn completed_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "Completed": {
                "execution_id": "exec-legacy",
                "completion_id": "COMP001",
                "phase": "phase-A",
                "agent": "old-agent",
                "at": "2026-04-25T03:00:00Z"
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::Completed {
                ref completion_id,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
                ..
            } => {
                assert_eq!(completion_id, "COMP001");
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected Completed"),
        }
    }

    /// `Completed` without dispatch metadata must serialize byte-identical
    /// to the old 5-field wire form.
    #[test]
    fn completed_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::Completed {
            execution_id: "e".into(),
            completion_id: "COMP001".into(),
            phase: "p".into(),
            agent: "a".into(),
            at: "2026-04-25T00:00:00Z".into(),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let completed = parsed
            .get("Completed")
            .and_then(|v| v.as_object())
            .expect("Completed payload");
        assert!(!completed.contains_key("dispatch_strategy"));
        assert!(!completed.contains_key("target_project"));
        assert!(!completed.contains_key("requested_cwd"));
        for key in ["execution_id", "completion_id", "phase", "agent", "at"] {
            assert!(completed.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(completed.len(), 5);
    }

    /// `Completed` with dispatch metadata round-trips and re-serializes
    /// exactly the keys present.
    #[test]
    fn completed_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::Completed {
            execution_id: "exec-disp".into(),
            completion_id: "COMP002".into(),
            phase: "phase-B".into(),
            agent: "claude".into(),
            at: "2026-04-25T04:00:00Z".into(),
            dispatch_strategy: Some("fresh-code-alignment".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let completed = parsed
            .get("Completed")
            .and_then(|v| v.as_object())
            .expect("Completed payload");
        assert_eq!(completed.len(), 8);
        assert_eq!(
            completed.get("dispatch_strategy").and_then(|v| v.as_str()),
            Some("fresh-code-alignment")
        );
        assert_eq!(
            completed.get("target_project").and_then(|v| v.as_str()),
            Some("missiond")
        );
        assert_eq!(
            completed.get("requested_cwd").and_then(|v| v.as_str()),
            Some("/Users/x/Projects/missiond")
        );
    }

    /// Partial-metadata producers — only `dispatch_strategy` known —
    /// must round-trip on both `Claimed` and `Completed` and skip the
    /// absent siblings.
    #[test]
    fn claimed_and_completed_partial_metadata_skips_absent_siblings() {
        let claimed = ExecutionEvent::Claimed {
            execution_id: "exec-part".into(),
            claim_id: "C003".into(),
            claimer: "claude".into(),
            scope: "src/".into(),
            phase: "".into(),
            lease_expires_at: "2026-04-25T05:00:00Z".into(),
            dispatch_strategy: Some("resident-lisp".into()),
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&claimed).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(claimed, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed.get("Claimed").and_then(|v| v.as_object()).unwrap();
        assert!(payload.contains_key("dispatch_strategy"));
        assert!(!payload.contains_key("target_project"));
        assert!(!payload.contains_key("requested_cwd"));
        assert_eq!(payload.len(), 7);

        let completed = ExecutionEvent::Completed {
            execution_id: "exec-part".into(),
            completion_id: "COMP003".into(),
            phase: "phase-X".into(),
            agent: "claude".into(),
            at: "2026-04-25T06:00:00Z".into(),
            dispatch_strategy: Some("resident-lisp".into()),
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&completed).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(completed, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed.get("Completed").and_then(|v| v.as_object()).unwrap();
        assert!(payload.contains_key("dispatch_strategy"));
        assert!(!payload.contains_key("target_project"));
        assert!(!payload.contains_key("requested_cwd"));
        assert_eq!(payload.len(), 6);
    }

    /// Producers may surface only one of the three optional fields. Each
    /// such partial form must round-trip and skip the absent siblings.
    #[test]
    fn opened_with_partial_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::Opened {
            execution_id: "exec-part".into(),
            parent_design: "p.lisp".into(),
            scope: "s".into(),
            owner: "claude".into(),
            path: ".missiond/v2/exec-part.lisp".into(),
            dispatch_strategy: Some("resident-lisp".into()),
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let opened = parsed
            .get("Opened")
            .and_then(|v| v.as_object())
            .expect("Opened payload");
        assert!(opened.contains_key("dispatch_strategy"));
        assert!(!opened.contains_key("target_project"));
        assert!(!opened.contains_key("requested_cwd"));
        assert_eq!(opened.len(), 6);
    }
}
