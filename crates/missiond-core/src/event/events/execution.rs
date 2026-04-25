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
    Claimed {
        execution_id: String,
        claim_id: String,
        claimer: String,
        scope: String,
        phase: String,
        lease_expires_at: String,
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
    Completed {
        execution_id: String,
        completion_id: String,
        phase: String,
        agent: String,
        at: String,
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
