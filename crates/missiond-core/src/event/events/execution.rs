//! `ExecutionEvent` — live projection of `mission_execution`'s
//! agent-execution-coordination companion log.
//!
//! Lisp authority:
//!   - intent-event-bus.lisp :: planned-event-extensions :: ExecutionEvent
//!     (:candidate-domain "Execution")
//!   - intent-flow.lisp :: F-execution-log-governance
//!   - intent-memory.lisp :: helper agent-execution-coordination v0.5.x
//!
//! Durable evidence remains the on-disk
//! `<project_root>/.missiond/v3/runtime/executions/<id>.lisp` companion file
//! (with `.missiond/v2/<id>.lisp` legacy fallback). The bus event is a
//! non-authoritative live notification —
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
    /// on-disk `<project_root>/.missiond/v3/runtime/executions/<id>.lisp`
    /// companion file; the event metadata is a live projection for status /
    /// audit consumers.
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
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block (intent-worker.lisp :: claudecode-workstation-orchestration ::
    /// execution-strategy-record). Inherited from the on-disk meta on each
    /// emit so a long-lived heartbeat stream stays correlatable against
    /// the dispatch context without re-loading the companion log. Optional
    /// + skipped on serialize when absent so legacy producers / consumers
    /// stay byte-identical with the original 5-field wire form.
    Heartbeat {
        execution_id: String,
        claim_id: String,
        claimer: String,
        heartbeat_at: String,
        lease_expires_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// Claim was released by its owner (clean handoff).
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block. Released completes the pair with `Claimed`, so consumers
    /// computing claim-lifetime aggregates can join both events without
    /// re-loading the companion log. Optional + skipped on serialize when
    /// absent so legacy producers / consumers stay byte-identical with the
    /// original 5-field wire form.
    Released {
        execution_id: String,
        claim_id: String,
        claimer: String,
        released_at: String,
        summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// A deviation between Lisp design and observed reality was recorded.
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block. Optional + skipped on serialize when absent so legacy
    /// producers / consumers stay byte-identical with the original
    /// 4-field wire form.
    DeviationRecorded {
        execution_id: String,
        deviation_id: String,
        phase: String,
        approved_by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// A design decision was recorded against the execution.
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block. Optional + skipped on serialize when absent so legacy
    /// producers / consumers stay byte-identical with the original
    /// 4-field wire form.
    DecisionRecorded {
        execution_id: String,
        decision_id: String,
        decided_by: String,
        at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// A blocking/non-blocking issue was raised against the execution.
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block. Optional + skipped on serialize when absent so legacy
    /// producers / consumers stay byte-identical with the original
    /// 4-field wire form.
    IssueRecorded {
        execution_id: String,
        issue_id: String,
        severity: String,
        owner: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
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
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block so audit-report consumers can route on dispatch context
    /// without re-loading the file. Optional + skipped on serialize when
    /// absent so legacy producers / consumers stay byte-identical with the
    /// original 4-field wire form.
    Audited {
        execution_id: String,
        ok: bool,
        findings_count: u32,
        error_count: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// A repair pass ran — `applied=false` means dry-run preview only.
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block. Optional + skipped on serialize when absent so legacy
    /// producers / consumers stay byte-identical with the original 3-field
    /// wire form.
    Repaired {
        execution_id: String,
        applied: bool,
        action_count: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// The audit / repair pass detected an active claim whose lease has
    /// expired without release. Emitted standalone so notifiers can react
    /// without scanning the full audit findings.
    ///
    /// `dispatch_strategy` / `target_project` / `requested_cwd` mirror the
    /// workstation-dispatch metadata persisted in the companion log meta
    /// block so a stale-claim notifier can route on dispatch context
    /// without re-loading the file. Optional + skipped on serialize when
    /// absent so legacy producers / consumers stay byte-identical with the
    /// original 4-field wire form.
    StaleClaim {
        execution_id: String,
        claim_id: String,
        claimer: String,
        lease_expires_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatch_strategy: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_cwd: Option<String>,
    },
    /// Per-node lifecycle transition observed by the PLAN DAG runtime v2
    /// (`plan_dag.rs`). Bus is non-authoritative — durable evidence still
    /// lives in `<project_root>/.missiond/v3/runtime/plans/<plan_id>.evidence.json`
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
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::Released {
                execution_id: "e".into(),
                claim_id: "C001".into(),
                claimer: "a".into(),
                released_at: "2026-04-25T00:00:00Z".into(),
                summary: None,
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::DeviationRecorded {
                execution_id: "e".into(),
                deviation_id: "D001".into(),
                phase: "p".into(),
                approved_by: "auto".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::DecisionRecorded {
                execution_id: "e".into(),
                decision_id: "DC001".into(),
                decided_by: "a".into(),
                at: "2026-04-25T00:00:00Z".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::IssueRecorded {
                execution_id: "e".into(),
                issue_id: "I001".into(),
                severity: "medium".into(),
                owner: "".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
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
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::Repaired {
                execution_id: "e".into(),
                applied: false,
                action_count: 0,
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::StaleClaim {
                execution_id: "e".into(),
                claim_id: "C001".into(),
                claimer: "a".into(),
                lease_expires_at: "2026-04-25T00:00:00Z".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
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
                path: ".missiond/v3/runtime/executions/exec-1.lisp".into(),
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::Opened {
                execution_id: "exec-2".into(),
                parent_design: "intent-worker.lisp".into(),
                scope: "src/handlers/**".into(),
                owner: "claude".into(),
                path: ".missiond/v3/runtime/executions/exec-2.lisp".into(),
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
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
            },
            ExecutionEvent::Audited {
                execution_id: "exec-1".into(),
                ok: false,
                findings_count: 3,
                error_count: 1,
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
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
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
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
            path: ".missiond/v3/runtime/executions/exec-disp.lisp".into(),
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
        for key in [
            "target",
            "dispatch_strategy",
            "target_project",
            "attempt",
            "reason",
        ] {
            assert!(
                !payload.contains_key(key),
                "absent optional {} must skip-serialize",
                key
            );
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
            path: ".missiond/v3/runtime/executions/exec-part.lisp".into(),
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

    // ── Wave 20 / Task 09 — legacy variant dispatch metadata sweep ──
    //
    // The Opened / Claimed / Completed / PlanNodeStateChanged variants
    // grew the workstation-dispatch trio in waves 11, 14, 18 and 19.
    // The remaining `Heartbeat / Released / DeviationRecorded /
    // DecisionRecorded / IssueRecorded / Audited / Repaired / StaleClaim`
    // variants now share the same projection so every consumer can route
    // on dispatch context without re-loading the companion log.
    //
    // Backward compatibility is mandatory: legacy JSON without the trio
    // must still deserialize to `None` slots, and producers that omit the
    // trio must serialize byte-identical to the original wire form.
    //
    // For each swept variant we pin two invariants:
    //   1) `*_deserializes_legacy_json_without_dispatch_metadata` — old
    //      JSON produced before the trio was added still parses with the
    //      new optional slots filled with `None`.
    //   2) `*_with_dispatch_metadata_round_trips` — when populated, every
    //      provided value round-trips verbatim and the wire form gains
    //      exactly the supplied keys.
    // Together these match the Opened / Claimed / Completed pattern and
    // give CI a deterministic guard against accidentally re-adding the
    // skip-serialize attribute regression.

    /// Helper: assert a JSON object lacks all three dispatch keys.
    fn assert_no_dispatch_keys(map: &serde_json::Map<String, serde_json::Value>) {
        assert!(!map.contains_key("dispatch_strategy"));
        assert!(!map.contains_key("target_project"));
        assert!(!map.contains_key("requested_cwd"));
    }

    /// Helper: assert a JSON object carries all three dispatch keys with
    /// the expected values.
    fn assert_dispatch_keys(
        map: &serde_json::Map<String, serde_json::Value>,
        strategy: &str,
        project: &str,
        cwd: &str,
    ) {
        assert_eq!(
            map.get("dispatch_strategy").and_then(|v| v.as_str()),
            Some(strategy)
        );
        assert_eq!(
            map.get("target_project").and_then(|v| v.as_str()),
            Some(project)
        );
        assert_eq!(map.get("requested_cwd").and_then(|v| v.as_str()), Some(cwd));
    }

    // ── Heartbeat ────────────────────────────────────────────────────

    #[test]
    fn heartbeat_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "Heartbeat": {
                "execution_id": "exec-legacy",
                "claim_id": "C001",
                "claimer": "old-claimer",
                "heartbeat_at": "2026-04-25T00:00:00Z",
                "lease_expires_at": "2026-04-25T00:30:00Z"
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::Heartbeat {
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
            _ => panic!("expected Heartbeat"),
        }
    }

    #[test]
    fn heartbeat_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::Heartbeat {
            execution_id: "e".into(),
            claim_id: "C001".into(),
            claimer: "a".into(),
            heartbeat_at: "2026-04-25T00:00:00Z".into(),
            lease_expires_at: "2026-04-25T00:30:00Z".into(),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("Heartbeat")
            .and_then(|v| v.as_object())
            .expect("Heartbeat payload");
        assert_no_dispatch_keys(payload);
        for key in [
            "execution_id",
            "claim_id",
            "claimer",
            "heartbeat_at",
            "lease_expires_at",
        ] {
            assert!(payload.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(payload.len(), 5);
    }

    #[test]
    fn heartbeat_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::Heartbeat {
            execution_id: "exec-disp".into(),
            claim_id: "C002".into(),
            claimer: "claude".into(),
            heartbeat_at: "2026-04-25T01:00:00Z".into(),
            lease_expires_at: "2026-04-25T01:30:00Z".into(),
            dispatch_strategy: Some("agent-team".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed.get("Heartbeat").and_then(|v| v.as_object()).unwrap();
        assert_eq!(payload.len(), 8);
        assert_dispatch_keys(
            payload,
            "agent-team",
            "missiond",
            "/Users/x/Projects/missiond",
        );
    }

    // ── Released ─────────────────────────────────────────────────────

    #[test]
    fn released_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "Released": {
                "execution_id": "exec-legacy",
                "claim_id": "C001",
                "claimer": "old-claimer",
                "released_at": "2026-04-25T00:00:00Z",
                "summary": null
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::Released {
                ref execution_id,
                ref summary,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
                ..
            } => {
                assert_eq!(execution_id, "exec-legacy");
                assert!(summary.is_none());
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected Released"),
        }
    }

    #[test]
    fn released_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::Released {
            execution_id: "e".into(),
            claim_id: "C001".into(),
            claimer: "a".into(),
            released_at: "2026-04-25T00:00:00Z".into(),
            summary: Some("done".into()),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("Released")
            .and_then(|v| v.as_object())
            .expect("Released payload");
        assert_no_dispatch_keys(payload);
        // `summary` is `Option<String>` without skip-serializing so it must
        // always land on the wire (legacy 5-field shape includes it).
        for key in [
            "execution_id",
            "claim_id",
            "claimer",
            "released_at",
            "summary",
        ] {
            assert!(payload.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(payload.len(), 5);
    }

    #[test]
    fn released_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::Released {
            execution_id: "exec-disp".into(),
            claim_id: "C002".into(),
            claimer: "claude".into(),
            released_at: "2026-04-25T02:00:00Z".into(),
            summary: Some("phase-A done".into()),
            dispatch_strategy: Some("fresh-code-alignment".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed.get("Released").and_then(|v| v.as_object()).unwrap();
        assert_eq!(payload.len(), 8);
        assert_dispatch_keys(
            payload,
            "fresh-code-alignment",
            "missiond",
            "/Users/x/Projects/missiond",
        );
    }

    // ── DeviationRecorded ────────────────────────────────────────────

    #[test]
    fn deviation_recorded_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "DeviationRecorded": {
                "execution_id": "exec-legacy",
                "deviation_id": "D001",
                "phase": "phase-A",
                "approved_by": "auto"
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::DeviationRecorded {
                ref deviation_id,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
                ..
            } => {
                assert_eq!(deviation_id, "D001");
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected DeviationRecorded"),
        }
    }

    #[test]
    fn deviation_recorded_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::DeviationRecorded {
            execution_id: "e".into(),
            deviation_id: "D001".into(),
            phase: "p".into(),
            approved_by: "auto".into(),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("DeviationRecorded")
            .and_then(|v| v.as_object())
            .expect("DeviationRecorded payload");
        assert_no_dispatch_keys(payload);
        for key in ["execution_id", "deviation_id", "phase", "approved_by"] {
            assert!(payload.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(payload.len(), 4);
    }

    #[test]
    fn deviation_recorded_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::DeviationRecorded {
            execution_id: "exec-disp".into(),
            deviation_id: "D010".into(),
            phase: "phase-B".into(),
            approved_by: "claude".into(),
            dispatch_strategy: Some("agent-team".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond/crates".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("DeviationRecorded")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(payload.len(), 7);
        assert_dispatch_keys(
            payload,
            "agent-team",
            "missiond",
            "/Users/x/Projects/missiond/crates",
        );
    }

    // ── DecisionRecorded ─────────────────────────────────────────────

    #[test]
    fn decision_recorded_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "DecisionRecorded": {
                "execution_id": "exec-legacy",
                "decision_id": "DC001",
                "decided_by": "claude",
                "at": "2026-04-25T00:00:00Z"
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::DecisionRecorded {
                ref decision_id,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
                ..
            } => {
                assert_eq!(decision_id, "DC001");
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected DecisionRecorded"),
        }
    }

    #[test]
    fn decision_recorded_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::DecisionRecorded {
            execution_id: "e".into(),
            decision_id: "DC001".into(),
            decided_by: "a".into(),
            at: "2026-04-25T00:00:00Z".into(),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("DecisionRecorded")
            .and_then(|v| v.as_object())
            .expect("DecisionRecorded payload");
        assert_no_dispatch_keys(payload);
        for key in ["execution_id", "decision_id", "decided_by", "at"] {
            assert!(payload.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(payload.len(), 4);
    }

    #[test]
    fn decision_recorded_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::DecisionRecorded {
            execution_id: "exec-disp".into(),
            decision_id: "DC010".into(),
            decided_by: "claude".into(),
            at: "2026-04-25T05:00:00Z".into(),
            dispatch_strategy: Some("resident-lisp".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("DecisionRecorded")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(payload.len(), 7);
        assert_dispatch_keys(
            payload,
            "resident-lisp",
            "missiond",
            "/Users/x/Projects/missiond",
        );
    }

    // ── IssueRecorded ────────────────────────────────────────────────

    #[test]
    fn issue_recorded_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "IssueRecorded": {
                "execution_id": "exec-legacy",
                "issue_id": "I001",
                "severity": "medium",
                "owner": "claude"
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::IssueRecorded {
                ref issue_id,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
                ..
            } => {
                assert_eq!(issue_id, "I001");
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected IssueRecorded"),
        }
    }

    #[test]
    fn issue_recorded_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::IssueRecorded {
            execution_id: "e".into(),
            issue_id: "I001".into(),
            severity: "medium".into(),
            owner: "".into(),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("IssueRecorded")
            .and_then(|v| v.as_object())
            .expect("IssueRecorded payload");
        assert_no_dispatch_keys(payload);
        for key in ["execution_id", "issue_id", "severity", "owner"] {
            assert!(payload.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(payload.len(), 4);
    }

    #[test]
    fn issue_recorded_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::IssueRecorded {
            execution_id: "exec-disp".into(),
            issue_id: "I010".into(),
            severity: "high".into(),
            owner: "claude".into(),
            dispatch_strategy: Some("agent-team".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("IssueRecorded")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(payload.len(), 7);
        assert_dispatch_keys(
            payload,
            "agent-team",
            "missiond",
            "/Users/x/Projects/missiond",
        );
    }

    // ── Audited ──────────────────────────────────────────────────────

    #[test]
    fn audited_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "Audited": {
                "execution_id": "exec-legacy",
                "ok": false,
                "findings_count": 3,
                "error_count": 1
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::Audited {
                ref execution_id,
                ok,
                findings_count,
                error_count,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
            } => {
                assert_eq!(execution_id, "exec-legacy");
                assert!(!ok);
                assert_eq!(findings_count, 3);
                assert_eq!(error_count, 1);
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected Audited"),
        }
    }

    #[test]
    fn audited_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::Audited {
            execution_id: "e".into(),
            ok: true,
            findings_count: 0,
            error_count: 0,
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("Audited")
            .and_then(|v| v.as_object())
            .expect("Audited payload");
        assert_no_dispatch_keys(payload);
        for key in ["execution_id", "ok", "findings_count", "error_count"] {
            assert!(payload.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(payload.len(), 4);
    }

    #[test]
    fn audited_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::Audited {
            execution_id: "exec-disp".into(),
            ok: false,
            findings_count: 2,
            error_count: 1,
            dispatch_strategy: Some("fresh-code-alignment".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed.get("Audited").and_then(|v| v.as_object()).unwrap();
        assert_eq!(payload.len(), 7);
        assert_dispatch_keys(
            payload,
            "fresh-code-alignment",
            "missiond",
            "/Users/x/Projects/missiond",
        );
    }

    // ── Repaired ─────────────────────────────────────────────────────

    #[test]
    fn repaired_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "Repaired": {
                "execution_id": "exec-legacy",
                "applied": true,
                "action_count": 4
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::Repaired {
                ref execution_id,
                applied,
                action_count,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
            } => {
                assert_eq!(execution_id, "exec-legacy");
                assert!(applied);
                assert_eq!(action_count, 4);
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected Repaired"),
        }
    }

    #[test]
    fn repaired_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::Repaired {
            execution_id: "e".into(),
            applied: false,
            action_count: 0,
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("Repaired")
            .and_then(|v| v.as_object())
            .expect("Repaired payload");
        assert_no_dispatch_keys(payload);
        for key in ["execution_id", "applied", "action_count"] {
            assert!(payload.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(payload.len(), 3);
    }

    #[test]
    fn repaired_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::Repaired {
            execution_id: "exec-disp".into(),
            applied: true,
            action_count: 5,
            dispatch_strategy: Some("agent-team".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed.get("Repaired").and_then(|v| v.as_object()).unwrap();
        assert_eq!(payload.len(), 6);
        assert_dispatch_keys(
            payload,
            "agent-team",
            "missiond",
            "/Users/x/Projects/missiond",
        );
    }

    // ── StaleClaim ───────────────────────────────────────────────────

    #[test]
    fn stale_claim_deserializes_legacy_json_without_dispatch_metadata() {
        let legacy = r#"{
            "StaleClaim": {
                "execution_id": "exec-legacy",
                "claim_id": "C001",
                "claimer": "old-claimer",
                "lease_expires_at": "2026-04-25T00:00:00Z"
            }
        }"#;
        let ev: ExecutionEvent = serde_json::from_str(legacy).expect("legacy JSON parses");
        match ev {
            ExecutionEvent::StaleClaim {
                ref execution_id,
                ref claim_id,
                ref dispatch_strategy,
                ref target_project,
                ref requested_cwd,
                ..
            } => {
                assert_eq!(execution_id, "exec-legacy");
                assert_eq!(claim_id, "C001");
                assert!(dispatch_strategy.is_none());
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected StaleClaim"),
        }
    }

    #[test]
    fn stale_claim_without_dispatch_metadata_serializes_byte_identical_to_legacy() {
        let ev = ExecutionEvent::StaleClaim {
            execution_id: "e".into(),
            claim_id: "C001".into(),
            claimer: "a".into(),
            lease_expires_at: "2026-04-25T00:00:00Z".into(),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("StaleClaim")
            .and_then(|v| v.as_object())
            .expect("StaleClaim payload");
        assert_no_dispatch_keys(payload);
        for key in ["execution_id", "claim_id", "claimer", "lease_expires_at"] {
            assert!(payload.contains_key(key), "missing legacy key {}", key);
        }
        assert_eq!(payload.len(), 4);
    }

    #[test]
    fn stale_claim_with_dispatch_metadata_round_trips() {
        let ev = ExecutionEvent::StaleClaim {
            execution_id: "exec-disp".into(),
            claim_id: "C002".into(),
            claimer: "claude".into(),
            lease_expires_at: "2026-04-25T00:30:00Z".into(),
            dispatch_strategy: Some("resident-lisp".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/Users/x/Projects/missiond".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let payload = parsed
            .get("StaleClaim")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(payload.len(), 7);
        assert_dispatch_keys(
            payload,
            "resident-lisp",
            "missiond",
            "/Users/x/Projects/missiond",
        );
    }

    /// Partial-metadata producer (only `dispatch_strategy`) on every
    /// swept variant — proves the skip-serialize attribute is wired
    /// correctly per-field, not just collectively. One representative
    /// scan keeps CI cheap while still flagging a regression on any
    /// individual field.
    #[test]
    fn swept_variants_partial_metadata_skips_absent_siblings() {
        // Heartbeat
        let hb = ExecutionEvent::Heartbeat {
            execution_id: "e".into(),
            claim_id: "C001".into(),
            claimer: "a".into(),
            heartbeat_at: "t".into(),
            lease_expires_at: "t2".into(),
            dispatch_strategy: Some("agent-team".into()),
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&hb).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let p = parsed.get("Heartbeat").and_then(|v| v.as_object()).unwrap();
        assert!(p.contains_key("dispatch_strategy"));
        assert!(!p.contains_key("target_project"));
        assert!(!p.contains_key("requested_cwd"));
        assert_eq!(p.len(), 6);

        // Audited
        let au = ExecutionEvent::Audited {
            execution_id: "e".into(),
            ok: true,
            findings_count: 0,
            error_count: 0,
            dispatch_strategy: Some("agent-team".into()),
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&au).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let p = parsed.get("Audited").and_then(|v| v.as_object()).unwrap();
        assert!(p.contains_key("dispatch_strategy"));
        assert!(!p.contains_key("target_project"));
        assert!(!p.contains_key("requested_cwd"));
        assert_eq!(p.len(), 5);

        // Repaired
        let rp = ExecutionEvent::Repaired {
            execution_id: "e".into(),
            applied: false,
            action_count: 0,
            dispatch_strategy: Some("agent-team".into()),
            target_project: None,
            requested_cwd: None,
        };
        let json = serde_json::to_string(&rp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let p = parsed.get("Repaired").and_then(|v| v.as_object()).unwrap();
        assert!(p.contains_key("dispatch_strategy"));
        assert!(!p.contains_key("target_project"));
        assert!(!p.contains_key("requested_cwd"));
        assert_eq!(p.len(), 4);
    }
}
