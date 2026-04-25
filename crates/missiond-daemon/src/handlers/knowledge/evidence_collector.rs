//! evidence_collector — reusable plan-evidence sidecar entry builder.
//!
//! Lisp authority:
//!   - intent-flow.lisp ::
//!       F-intent-alignment-plan-execution-loop :: s6 execution-runner
//!       (plan-runner evidence trail)
//!   - intent-memory.lisp :: directive-layer ::
//!       file-first-artifacts :: plan-lisp (plan evidence sidecar lives next
//!       to the plan companion log; file-first SSOT)
//!   - intent-tools.lisp :: implemented-surface mission_plan ::
//!       record_evidence (evidence ingestion surface)
//!
//! Scope (wave-12 :: evidence-collector v0):
//!   - Centralise the evidence entry shape so plan.rs (`record_evidence`,
//!     `plan_runner_dispatch`) and plan_dag.rs (`plan_dag_node_dispatch`) all
//!     stamp the same canonical fields (`source` / `kind` / `schema_version`
//!     / `recorded_at`) without each call site re-deriving them.
//!   - Collect "what happened" structured pieces — inner dispatch result
//!     summary, verification command list/result, git diff stat / changed
//!     files, commit_hash + commit_status, ExecutionEvent references — into
//!     a typed builder so the caller chooses fields explicitly instead of
//!     hand-mixing JSON keys.
//!   - Wrap the existing file-first sidecar helper
//!     (`super::plan::append_plan_evidence_entry`) and surface a structured
//!     `AppendOutcome` so write failures stay visible (CLAUDE.md
//!     `feedback_fail_fast_no_fallback` — never silently swallow them).
//!   - Stay strictly file-first: NO new DB migration, NO new bus event. The
//!     sidecar JSON at `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json`
//!     remains the single source of truth.
//!
//! What this module deliberately does NOT do:
//!   - It does NOT subscribe to the event bus or read live ExecutionEvents.
//!     The collector accepts already-known event references (`event_id` /
//!     `source` / `kind`) from the caller. When no real event is available
//!     callers can use `EventRef::unavailable("…")` so the entry still records
//!     "we tried to correlate but couldn't" instead of pretending none were
//!     wanted.
//!   - It does NOT mutate plan FSM. Sidecar append is independent of plan
//!     status transitions; that authority remains with `action_execute`.
//!   - It does NOT replace `append_plan_evidence_entry` — it composes on top
//!     so existing direct callers keep working byte-for-byte. This keeps the
//!     v0 surface additive: anyone writing legacy `{ "evidence": …, "kind":
//!     "…" }` JSON via the manager continues to land in the same sidecar.

use anyhow::{anyhow, Result};
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

use crate::state::AppState;

/// Companion sidecar directory under a project root. Mirrors the constant in
/// `plan.rs`; duplicated here so the lower-level
/// [`append_entry_to_project_root`] writer can be tested without taking an
/// `AppState` (which carries half the daemon).
const COMPANION_DIR: &str = ".missiond/v2/plans";

/// Schema version stamped onto every evidence entry produced through this
/// builder. Bump when the `EvidenceEntry` shape gains a non-additive field so
/// downstream consumers can route on it explicitly. Adding optional fields
/// does NOT require a bump; renaming / repurposing existing fields does.
pub(crate) const EVIDENCE_SCHEMA_VERSION: &str = "v0";

/// Canonical `source` tags surfaced in evidence entries. The legacy
/// `"plan_runner_dispatch"` / `"plan_dag_node_dispatch"` strings are kept as
/// the wire form so existing readers (audit dashboards, intent-event-bus
/// consumers, scoped-commit handoff metadata) stay byte-identical.
///
/// `record_evidence_manual` is the new tag for the `mission_plan(action=
/// record_evidence)` manual entry — the prior wire form had no `source`,
/// only a `kind`. Callers that want to keep emitting the legacy untagged
/// form can still use `EvidenceCollector::legacy_record_evidence` which
/// preserves the historical shape.
pub(crate) mod source {
    /// Manual `mission_plan(action=record_evidence)` entry written by an
    /// agent / human caller. Always treat this as un-vetted: the collector
    /// does no schema validation on the inner `evidence` payload.
    pub(crate) const RECORD_EVIDENCE_MANUAL: &str = "record_evidence_manual";

    /// Single-node v0 plan-runner internal dispatch (plan.rs ::
    /// `action_execute_internal`). Wire-compatible with the historical
    /// `kind="plan_runner_dispatch"` entries.
    pub(crate) const PLAN_RUNNER_DISPATCH: &str = "plan_runner_dispatch";

    /// Per-node DAG scheduler dispatch (plan_dag.rs).
    pub(crate) const PLAN_DAG_NODE_DISPATCH: &str = "plan_dag_node_dispatch";
}

/// Canonical `kind` taxonomy. We keep this open (callers can pass arbitrary
/// strings) but ship the well-known names as constants so the call sites
/// don't drift typos. Mirrors the historical sidecar shape:
///   - `dispatch`  : an inner handler was invoked, payload carries the
///                   `inner_result` / `inner_error` projection.
///   - `verification` : verification commands ran (test / lint / build) and
///                      we want to capture command list + summary + outcome.
///   - `git_diff` : git diff stat / changed-file list snapshot.
///   - `commit`   : commit hash / commit status handoff metadata.
///   - `note`     : free-form caller note (manual `record_evidence`).
pub(crate) mod kind {
    pub(crate) const DISPATCH: &str = "dispatch";
    pub(crate) const VERIFICATION: &str = "verification";
    pub(crate) const GIT_DIFF: &str = "git_diff";
    pub(crate) const COMMIT: &str = "commit";
    pub(crate) const NOTE: &str = "note";
}

/// Reference to an `ExecutionEvent` (or any other domain event) the caller
/// observed while producing this evidence entry.
///
/// `event_id` is whatever string identifier the producer has — for
/// `mission_execution` events that is the helper-protocol prefix-encoded id
/// (`C001`, `D003`, `COMP005`, …). `source` is the originating tool /
/// surface (e.g. `"mission_execution"`). `kind` is the event variant tag
/// (e.g. `"opened"`, `"completed"`).
///
/// When the caller knows an event SHOULD be referenced but cannot produce
/// one (the bus subscription is offline, the event id was lost, etc.) use
/// [`EventRef::unavailable`] — the entry then records `"unavailable": true`
/// plus a reason string so consumers can tell "no events" apart from "we
/// tried but failed to correlate".
#[derive(Debug, Clone)]
pub(crate) struct EventRef {
    pub event_id: Option<String>,
    pub source: Option<String>,
    pub kind: Option<String>,
    pub unavailable_reason: Option<String>,
}

impl EventRef {
    pub(crate) fn new(source: impl Into<String>, kind: impl Into<String>, event_id: impl Into<String>) -> Self {
        Self {
            event_id: Some(event_id.into()),
            source: Some(source.into()),
            kind: Some(kind.into()),
            unavailable_reason: None,
        }
    }

    /// Record a placeholder: caller wanted to attach an event reference but
    /// the live source isn't available. `reason` is mandatory so the
    /// resulting entry never silently shows up as "no events at all".
    pub(crate) fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            event_id: None,
            source: None,
            kind: None,
            unavailable_reason: Some(reason.into()),
        }
    }

    fn into_json(self) -> Value {
        let mut m = Map::new();
        if let Some(id) = self.event_id {
            m.insert("event_id".to_string(), Value::String(id));
        }
        if let Some(src) = self.source {
            m.insert("source".to_string(), Value::String(src));
        }
        if let Some(k) = self.kind {
            m.insert("kind".to_string(), Value::String(k));
        }
        if let Some(reason) = self.unavailable_reason {
            m.insert("unavailable".to_string(), Value::Bool(true));
            m.insert("unavailable_reason".to_string(), Value::String(reason));
        }
        Value::Object(m)
    }
}

/// Builder for a single evidence entry. Every field is optional; callers add
/// only what they have. The resulting JSON object always carries
/// `schema_version`, `source`, and `kind` (default `note` if unspecified).
#[derive(Debug, Clone)]
pub(crate) struct EvidenceEntry {
    source: String,
    kind: String,
    /// Free-form passthrough payload. Merged at the top level so legacy
    /// callers that already shape their own JSON keep working unchanged.
    extra: Map<String, Value>,
    inner_dispatch: Option<Value>,
    verification: Option<Value>,
    git_diff: Option<Value>,
    commit_metadata: Option<Value>,
    execution_events: Vec<EventRef>,
    /// State-transition annotation for DAG node entries (`ready -> succeeded`,
    /// `ready -> failed`, etc.). Optional; callers that don't have one omit it.
    state_transition: Option<String>,
}

impl EvidenceEntry {
    /// Start a new entry with explicit source + kind. Use the constants in
    /// [`source`] and [`kind`] for the well-known taxonomy; arbitrary strings
    /// are also accepted (the schema is intentionally open).
    pub(crate) fn new(source: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            kind: kind.into(),
            extra: Map::new(),
            inner_dispatch: None,
            verification: None,
            git_diff: None,
            commit_metadata: None,
            execution_events: Vec::new(),
            state_transition: None,
        }
    }

    /// Inner dispatch summary. Caller passes the already-projected
    /// `tool_result_payload(...)` value (or any JSON), the collector wraps
    /// it under `inner_dispatch` so the legacy `inner_result` /
    /// `inner_error` keys can coexist when callers add them via `with_extra`.
    pub(crate) fn with_inner_dispatch(mut self, summary: Value) -> Self {
        self.inner_dispatch = Some(summary);
        self
    }

    /// Verification commands run (tests / lint / build) — caller supplies a
    /// list of commands and a result summary; we record both verbatim.
    pub(crate) fn with_verification(mut self, commands: Vec<String>, result_summary: Value) -> Self {
        self.verification = Some(json!({
            "commands": commands,
            "result_summary": result_summary,
        }));
        self
    }

    /// Git diff snapshot — caller picks whatever shape they have
    /// (`git diff --stat` text, structured per-file list, etc.). Stored
    /// verbatim under `git_diff`.
    pub(crate) fn with_git_diff(mut self, payload: Value) -> Self {
        self.git_diff = Some(payload);
        self
    }

    /// Commit metadata — `commit_hash` is the resolved sha (40 hex chars or
    /// short form, caller's choice). `commit_status` is a free string (e.g.
    /// `"committed"` / `"detached_head"` / `"dirty"`).
    pub(crate) fn with_commit(mut self, commit_hash: Option<String>, commit_status: Option<String>) -> Self {
        let mut m = Map::new();
        if let Some(h) = commit_hash {
            m.insert("commit_hash".to_string(), Value::String(h));
        }
        if let Some(s) = commit_status {
            m.insert("commit_status".to_string(), Value::String(s));
        }
        if !m.is_empty() {
            self.commit_metadata = Some(Value::Object(m));
        }
        self
    }

    /// Append one ExecutionEvent reference. Caller can call this multiple
    /// times. To record "no event available" use `EventRef::unavailable(...)`
    /// rather than skipping the call — that distinction matters to consumers.
    pub(crate) fn add_execution_event(mut self, event: EventRef) -> Self {
        self.execution_events.push(event);
        self
    }

    /// Annotate a DAG node state transition (e.g. `"ready -> succeeded"`).
    pub(crate) fn with_state_transition(mut self, transition: impl Into<String>) -> Self {
        self.state_transition = Some(transition.into());
        self
    }

    /// Drop an arbitrary key/value into the entry. Used for fields not
    /// covered by typed setters (legacy passthrough — `target_tool`,
    /// `dispatch_strategy`, `node_id`, `plan_hint_summary`, etc.).
    pub(crate) fn with_extra(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }

    /// Bulk merge an existing JSON object into `extra`. Useful when a caller
    /// has already built a payload object (e.g. the legacy
    /// `plan_runner_dispatch` shape) and wants to migrate without rewriting
    /// every field by hand.
    pub(crate) fn merge_extra(mut self, value: Value) -> Self {
        if let Value::Object(m) = value {
            for (k, v) in m {
                // Avoid clobbering canonical fields the caller filled via
                // typed setters. Legacy keys override only if no typed
                // counterpart was set.
                match k.as_str() {
                    "schema_version" | "source" | "kind" | "recorded_at" => continue,
                    _ => {
                        self.extra.insert(k, v);
                    }
                }
            }
        }
        self
    }

    /// Render as a JSON value ready to hand to `append_plan_evidence_entry`.
    /// The `recorded_at` stamp is added by the underlying sidecar writer (so
    /// every entry — legacy or new — gets the same wall-clock semantics);
    /// the builder does NOT pre-stamp it here.
    pub(crate) fn into_json(self) -> Value {
        let Self {
            source,
            kind,
            extra,
            inner_dispatch,
            verification,
            git_diff,
            commit_metadata,
            execution_events,
            state_transition,
        } = self;

        let mut m = Map::new();
        m.insert("schema_version".to_string(), Value::String(EVIDENCE_SCHEMA_VERSION.to_string()));
        m.insert("source".to_string(), Value::String(source));
        m.insert("kind".to_string(), Value::String(kind));
        if let Some(t) = state_transition {
            m.insert("state_transition".to_string(), Value::String(t));
        }
        if let Some(v) = inner_dispatch {
            m.insert("inner_dispatch".to_string(), v);
        }
        if let Some(v) = verification {
            m.insert("verification".to_string(), v);
        }
        if let Some(v) = git_diff {
            m.insert("git_diff".to_string(), v);
        }
        if let Some(v) = commit_metadata {
            m.insert("commit".to_string(), v);
        }
        if !execution_events.is_empty() {
            let arr: Vec<Value> = execution_events.into_iter().map(EventRef::into_json).collect();
            m.insert("execution_events".to_string(), Value::Array(arr));
        }
        // Merge extra last so the canonical typed keys above always win
        // when both sides set the same field — the typed path is the
        // authoritative one.
        for (k, v) in extra {
            m.entry(k).or_insert(v);
        }
        Value::Object(m)
    }
}

/// Outcome of an evidence-sidecar append. Either `Written` (with the path +
/// final entry count returned by the underlying writer) or `Failed` (with the
/// underlying error text). Failures are NEVER silently swallowed — callers
/// are expected to surface them on the response payload (mirrors the
/// existing `evidence_error` field on plan-runner / DAG-runner responses).
#[derive(Debug, Clone)]
pub(crate) enum AppendOutcome {
    Written { path: PathBuf, entry_count: usize },
    Failed { error: String },
}

impl AppendOutcome {
    /// Convert to a `(path, error)` tuple matching the legacy plan.rs /
    /// plan_dag.rs response shape. Either field is None if the other applies.
    pub(crate) fn into_legacy_tuple(self) -> (Option<String>, Option<String>) {
        match self {
            AppendOutcome::Written { path, .. } => (Some(path.display().to_string()), None),
            AppendOutcome::Failed { error } => (None, Some(error)),
        }
    }
}

/// Wrapper around the existing `append_plan_evidence_entry` that takes a
/// typed [`EvidenceEntry`] and returns a structured [`AppendOutcome`].
///
/// Callers that already have an `AppState` + plan-resolution signals
/// (`project` / `cwd` / `target_project`) should use this. The wrapper keeps
/// the legacy `(Option<String>, Option<String>)` evidence_path/error shape
/// reachable via `AppendOutcome::into_legacy_tuple` for drop-in adoption.
pub(crate) async fn append(
    state: &AppState,
    plan_id: uuid::Uuid,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    entry: EvidenceEntry,
) -> AppendOutcome {
    let payload = entry.into_json();
    match super::plan::append_plan_evidence_entry(
        state,
        plan_id,
        project_arg,
        cwd_arg,
        target_project_arg,
        payload,
    )
    .await
    {
        Ok((path, count)) => AppendOutcome::Written { path, entry_count: count },
        Err(e) => AppendOutcome::Failed { error: e.to_string() },
    }
}

/// Lower-level writer that takes an already-resolved project root and
/// performs the same atomic-rename sidecar append as
/// `append_plan_evidence_entry`. Exists so unit tests can prove the
/// file-shape contract (multi-entry order, `schema_version` persistence,
/// recorded_at stamping) without spinning up a full `AppState`.
///
/// Keep this in lockstep with `super::plan::append_plan_evidence_entry`'s
/// on-disk shape — both write to the same canonical path (`<project_root>/
/// .missiond/v2/plans/<plan_id>.evidence.json`) so the two writers are
/// interchangeable from a reader's perspective.
pub(crate) fn append_entry_to_project_root(
    project_root: &Path,
    plan_id: uuid::Uuid,
    entry: Value,
) -> Result<(PathBuf, usize)> {
    let dir = project_root.join(COMPANION_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| anyhow!("mkdir {}: {}", dir.display(), e))?;
    let path = dir.join(format!("{}.evidence.json", plan_id));

    let mut bundle = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow!("read {}: {}", path.display(), e))?;
        serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({"plan_id": plan_id, "entries": []}))
    } else {
        json!({"plan_id": plan_id, "entries": []})
    };

    let stamped = match entry {
        Value::Object(mut map) => {
            map.insert("recorded_at".to_string(), json!(iso_now()));
            Value::Object(map)
        }
        other => json!({ "recorded_at": iso_now(), "evidence": other }),
    };

    if let Some(arr) = bundle.get_mut("entries").and_then(|v| v.as_array_mut()) {
        arr.push(stamped);
    } else {
        bundle["entries"] = json!([stamped]);
    }

    let entry_count = bundle
        .get("entries")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let body = serde_json::to_string_pretty(&bundle)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes()).map_err(|e| anyhow!("write tmp: {}", e))?;
    std::fs::rename(&tmp, &path).map_err(|e| anyhow!("rename: {}", e))?;
    Ok((path, entry_count))
}

fn iso_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Wrap the legacy `record_evidence` payload (`{"evidence": <opaque>}`) so
/// callers that want to migrate can stamp source/kind/schema_version on top
/// without losing the historical opaque body. Returns a JSON object the
/// caller passes straight to `append_plan_evidence_entry`.
///
/// `evidence_kind` defaults to [`kind::NOTE`] when caller does not pass one.
/// `source` defaults to [`source::RECORD_EVIDENCE_MANUAL`].
///
/// This keeps backward compatibility: the historical `{"evidence": ...}`
/// payload survives intact under the `evidence` key; the new top-level
/// stamps (`source`, `kind`, `schema_version`) are additive and existing
/// readers ignore unknown fields.
pub(crate) fn wrap_legacy_record_evidence(
    inner: Value,
    evidence_kind: Option<&str>,
    source_override: Option<&str>,
) -> Value {
    let mut m = Map::new();
    m.insert("schema_version".to_string(), Value::String(EVIDENCE_SCHEMA_VERSION.to_string()));
    m.insert(
        "source".to_string(),
        Value::String(source_override.unwrap_or(source::RECORD_EVIDENCE_MANUAL).to_string()),
    );
    m.insert(
        "kind".to_string(),
        Value::String(evidence_kind.unwrap_or(kind::NOTE).to_string()),
    );
    m.insert("evidence".to_string(), inner);
    Value::Object(m)
}

#[cfg(test)]
mod tests {
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
        assert_eq!(obj.get("verification").unwrap()["result_summary"]["passed"], true);
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
        assert!(commit.get("commit_hash").is_none(), "absent hash stays absent");
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
            .add_execution_event(EventRef::unavailable("bus subscription not yet wired"));
        let v = entry.into_json();
        let arr = v["execution_events"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["unavailable"], true);
        assert_eq!(arr[0]["unavailable_reason"], "bus subscription not yet wired");
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
        let entry = EvidenceEntry::new(source::PLAN_RUNNER_DISPATCH, kind::DISPATCH)
            .merge_extra(legacy);
        let v = entry.into_json();
        // Canonical stamps preserved.
        assert_eq!(v["schema_version"], EVIDENCE_SCHEMA_VERSION);
        assert_eq!(v["source"], source::PLAN_RUNNER_DISPATCH);
        assert_eq!(v["kind"], kind::DISPATCH);
        assert!(v.get("recorded_at").is_none(), "recorded_at stamped by writer, not builder");
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
            .with_verification(
                vec!["cargo test".to_string()],
                json!({"passed": true}),
            )
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
        assert!(entries[0].get("recorded_at").is_some(), "writer stamps recorded_at");
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
            let (_p, count) = append_entry_to_project_root(&root, plan_id, e.into_json())
                .expect("append succeeds");
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
        let entry = EvidenceEntry::new(source::PLAN_RUNNER_DISPATCH, kind::COMMIT)
            .with_commit(
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
}
