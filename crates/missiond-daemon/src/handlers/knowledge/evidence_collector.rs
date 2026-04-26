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
use missiond_core::event::events::ExecutionEvent;
use missiond_core::event::log::{LogReadable, Seq};
use missiond_core::event::Domain;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::state::AppState;

/// Companion sidecar directory under a project root. Mirrors the constant in
/// `plan.rs`; duplicated here so the lower-level
/// [`append_entry_to_project_root`] writer can be tested without taking an
/// `AppState` (which carries half the daemon).
///
/// `#[allow(dead_code)]`: only referenced by the `#[cfg(test)]`-only
/// [`append_entry_to_project_root`] writer. Production callers go through
/// `super::plan::append_plan_evidence_entry` which holds its own copy of the
/// path constant. We intentionally duplicate to avoid leaking that internal
/// const just for the test surface; the duplication is explicitly noted in
/// the docstring above.
#[allow(dead_code)]
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

    /// Workstation-dispatch v0 (workstation_dispatch.rs) — the conservative
    /// opt-in path that augments a `mission_task_delegate` call with a
    /// scoped task brief (objective / owned-files / forbidden-files /
    /// acceptance commands / commit policy). Distinguished from the bare
    /// `plan_runner_dispatch` source so audit consumers can tell when the
    /// task brief was injected and when only the legacy passthrough ran.
    pub(crate) const WORKSTATION_DISPATCH: &str = "workstation_dispatch";
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
    /// `#[allow(dead_code)]`: future plan-runner verification step (cargo
    /// test / lint / build summary) — wave-12 reserved this slot for the
    /// upcoming verification-evidence wiring (intent-flow.lisp ::
    /// F-intent-alignment-plan-execution-loop :: s7 verification-runner).
    /// Not yet emitted by any call site, but documented in the public
    /// taxonomy so the wire contract is stable when wiring lands.
    #[allow(dead_code)]
    pub(crate) const VERIFICATION: &str = "verification";
    /// `#[allow(dead_code)]`: future git-diff snapshot for plan evidence
    /// (paired with VERIFICATION above; the verification runner attaches a
    /// `git diff --stat` payload alongside the test results).
    #[allow(dead_code)]
    pub(crate) const GIT_DIFF: &str = "git_diff";
    /// `#[allow(dead_code)]`: future scoped-commit handoff metadata (wave-12
    /// task-01 commit_hash / commit_status round-trip — covered by
    /// `commit_metadata_round_trip_via_typed_setter` test).
    #[allow(dead_code)]
    pub(crate) const COMMIT: &str = "commit";
    pub(crate) const NOTE: &str = "note";
}

/// Provenance tag for an [`EventRef`]. Surfaces on the JSON envelope so
/// downstream consumers can tell at a glance whether the id came from the
/// live publish path, was recovered out-of-band from the in-memory cache /
/// log query (wave-16 / task 07 resolver), or could not be correlated at all.
///
/// Wire form is the all-lowercase variant tag (`"live"` / `"log"` /
/// `"unavailable"`) — kept stable so audit dashboards can pivot on a single
/// string without parsing structured discriminants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventRefStatus {
    /// Producer obtained the id directly from a successful publish (e.g.
    /// `BusServices::publish_execution_with_seq` returned the `Seq`) or
    /// from a deterministic correlation key the producer itself owns.
    Live,
    /// Recovered from a passive subscriber cache or a log-query lookup
    /// after the original publish path no longer carried the id. The id is
    /// still real — the recovery just happened post-hoc.
    Log,
    /// Caller wanted to attach an id but neither the publish path nor the
    /// resolver could surface one. The entry records the attempt + reason
    /// so consumers can tell "no events" apart from "we tried but failed
    /// to correlate".
    Unavailable,
}

impl EventRefStatus {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            EventRefStatus::Live => "live",
            EventRefStatus::Log => "log",
            EventRefStatus::Unavailable => "unavailable",
        }
    }
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
///
/// Wave-16 / task 07 added the [`EventRef::status`] tag. Existing
/// [`EventRef::new`] callers stay byte-compatible — `new(...)` is now an
/// alias for [`EventRef::live`] (the publish-path producers always know
/// they have a live id when they call it). The `status` field surfaces on
/// the JSON envelope as `"status": "live" | "log" | "unavailable"` so
/// downstream consumers can pivot without re-deriving provenance.
#[derive(Debug, Clone)]
pub(crate) struct EventRef {
    pub event_id: Option<String>,
    pub source: Option<String>,
    pub kind: Option<String>,
    pub unavailable_reason: Option<String>,
    pub status: EventRefStatus,
}

impl EventRef {
    /// Typed constructor for "I have a real event id" case. Wave-14 / Task
    /// 02 wired the PLAN DAG runtime v2 (`plan_dag.rs`) to call this with
    /// either the live `Seq` returned from
    /// `BusServices::publish_execution_with_seq(...)` or the deterministic
    /// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` fallback id
    /// when the bus publish fails. Plan-runner v0 (`plan.rs`) keeps using
    /// `EventRef::unavailable(...)` because the single-node runner does not
    /// yet propagate the inner `mission_execution` seq back to the
    /// evidence-write call site — that wiring is a separate follow-up.
    ///
    /// Wave-16 / task 07: alias for [`EventRef::live`]. Kept on the public
    /// surface for byte-compat with the wave-13/14 call sites and tests
    /// that already pin `EventRef::new(...)`.
    pub(crate) fn new(source: impl Into<String>, kind: impl Into<String>, event_id: impl Into<String>) -> Self {
        Self::live(source, kind, event_id)
    }

    /// Record a live event id (publish path returned it directly).
    pub(crate) fn live(source: impl Into<String>, kind: impl Into<String>, event_id: impl Into<String>) -> Self {
        Self {
            event_id: Some(event_id.into()),
            source: Some(source.into()),
            kind: Some(kind.into()),
            unavailable_reason: None,
            status: EventRefStatus::Live,
        }
    }

    /// Record an event id recovered from a post-hoc lookup (subscriber
    /// cache or log query). Same wire shape as [`EventRef::live`] except
    /// the `status` tag flips to `"log"` so consumers can tell the id was
    /// resolved out-of-band.
    ///
    /// `#[allow(dead_code)]`: wave-16 / task 07 introduced the resolver
    /// surface for downstream call sites that want post-hoc correlation.
    /// No production caller writes `from_log` directly today — the only
    /// producer is the resolver itself (see `EventRefResolver::lookup_*`).
    /// Kept on the public surface so future call sites that want to stamp
    /// a log-recovered id without going through the resolver have a typed
    /// entry point. Exercised by `event_ref_log_status_round_trips`.
    #[allow(dead_code)]
    pub(crate) fn from_log(source: impl Into<String>, kind: impl Into<String>, event_id: impl Into<String>) -> Self {
        Self {
            event_id: Some(event_id.into()),
            source: Some(source.into()),
            kind: Some(kind.into()),
            unavailable_reason: None,
            status: EventRefStatus::Log,
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
            status: EventRefStatus::Unavailable,
        }
    }

    fn into_json(self) -> Value {
        let mut m = Map::new();
        m.insert(
            "status".to_string(),
            Value::String(self.status.as_wire().to_string()),
        );
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
    /// Wave-17 / task 06 — primary event-ref summary surfaced at the top of
    /// the evidence entry (mirrors the leading `EventRef`'s status /
    /// source so audit consumers can pivot without diving into the
    /// `execution_events` array). Set via [`Self::with_primary_event_ref`].
    event_ref_status: Option<String>,
    event_ref_source: Option<String>,
    event_ref_warning: Option<String>,
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
            event_ref_status: None,
            event_ref_source: None,
            event_ref_warning: None,
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
    ///
    /// `#[allow(dead_code)]`: future verification-runner step (see
    /// `kind::VERIFICATION` docstring above). Exercised by
    /// `typed_setters_land_under_canonical_keys` test.
    #[allow(dead_code)]
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
    ///
    /// `#[allow(dead_code)]`: paired with `with_verification` above; the
    /// verification runner attaches a git-diff snapshot. Exercised by
    /// `typed_setters_land_under_canonical_keys` test.
    #[allow(dead_code)]
    pub(crate) fn with_git_diff(mut self, payload: Value) -> Self {
        self.git_diff = Some(payload);
        self
    }

    /// Commit metadata — `commit_hash` is the resolved sha (40 hex chars or
    /// short form, caller's choice). `commit_status` is a free string (e.g.
    /// `"committed"` / `"detached_head"` / `"dirty"`).
    ///
    /// `#[allow(dead_code)]`: scoped-commit handoff metadata is the
    /// canonical typed slot for the commit_hash/commit_status round-trip
    /// added in wave-12 task-01. Today plan-runner / DAG scheduler do not
    /// produce a scoped commit themselves (they hand off to the caller),
    /// but the typed setter is on the public surface so the upcoming
    /// scoped-commit wiring can drop it in. Exercised by
    /// `commit_metadata_round_trip_via_typed_setter`.
    #[allow(dead_code)]
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

    /// Wave-17 / task 06 — surface the primary event-ref provenance at the
    /// top level of the evidence entry. Mirrors what `add_execution_event`
    /// would record on the leading `EventRef` so audit consumers can pivot
    /// on `event_ref_status` / `event_ref_source` without iterating the
    /// `execution_events` array.
    ///
    /// `warning` is emitted on the JSON envelope only when present (e.g.
    /// "log query error: …" when the resolver had to fall back from the
    /// log path to `unavailable`).
    pub(crate) fn with_primary_event_ref(
        mut self,
        event_ref: &EventRef,
        warning: Option<String>,
    ) -> Self {
        self.event_ref_status = Some(event_ref.status.as_wire().to_string());
        self.event_ref_source = event_ref.source.clone();
        self.event_ref_warning = warning.or_else(|| {
            // Surface the unavailable_reason as the warning when no other
            // explicit warning was passed in — keeps the failure surface
            // visible without requiring the caller to duplicate it.
            if matches!(event_ref.status, EventRefStatus::Unavailable) {
                event_ref.unavailable_reason.clone()
            } else {
                None
            }
        });
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
    ///
    /// `#[allow(dead_code)]`: wave-13 plan.rs / plan_dag.rs migrated to
    /// per-field `with_extra(...)` calls (more explicit + easier to grep
    /// for the legacy passthrough keys). `merge_extra` stays on the public
    /// surface for the next legacy producer that wants to migrate without
    /// rewriting. Exercised by `merge_extra_skips_canonical_stamps`,
    /// `typed_inner_dispatch_wins_over_extra_legacy_inner_dispatch`, and
    /// `commit_metadata_round_trip_via_legacy_merge`.
    #[allow(dead_code)]
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
            event_ref_status,
            event_ref_source,
            event_ref_warning,
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
        if let Some(s) = event_ref_status {
            m.insert("event_ref_status".to_string(), Value::String(s));
        }
        if let Some(s) = event_ref_source {
            m.insert("event_ref_source".to_string(), Value::String(s));
        }
        if let Some(w) = event_ref_warning {
            m.insert("event_ref_warning".to_string(), Value::String(w));
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
///
/// `entry_count` is preserved on the `Written` variant for upcoming UI /
/// retrospective surfaces that want to show "this dispatch is the Nth entry
/// in the evidence trail" (today's plan-runner / DAG-runner responses only
/// surface the path). `into_legacy_tuple` discards it because the existing
/// `evidence_path` / `evidence_error` response shape predates per-entry
/// counting.
#[derive(Debug, Clone)]
pub(crate) enum AppendOutcome {
    Written {
        path: PathBuf,
        /// `#[allow(dead_code)]`: read by tests only today — see variant
        /// docstring for the future read-out plan.
        #[allow(dead_code)]
        entry_count: usize,
    },
    Failed {
        error: String,
    },
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
///
/// `#[allow(dead_code)]`: only invoked by `#[cfg(test)]` tests in this
/// module (`sidecar_append_preserves_order_and_schema_version`,
/// `sidecar_append_surfaces_writer_failure`,
/// `sidecar_append_is_strictly_additive`). Production callers go through
/// `append(...)` which delegates to `super::plan::append_plan_evidence_entry`
/// (resolves project root via the canonical resolver). This twin exists so
/// the on-disk shape contract is testable without standing up a full
/// `AppState` + project registry.
#[allow(dead_code)]
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

/// `#[allow(dead_code)]`: only called by [`append_entry_to_project_root`]
/// (test-only writer; see its docstring). Production callers reach the same
/// stamping behaviour through `super::plan::append_plan_evidence_entry`
/// which keeps a private copy.
#[allow(dead_code)]
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

// ═════════════════════════════════════════════════════════════════════════
// wave-16 / task 07 — EventRefResolver
//
// Conservative passive-cache lookup so call sites that no longer carry the
// live `Seq` from the original publish path can still attach a real event
// id when a subscriber observed the same event.
//
// Strategy chosen (lowest risk):
//   * In-memory bounded HashMap keyed by deterministic correlation tuple
//     (currently only `plan-node` lifecycle transitions; future kinds add
//     their own correlation key constructor).
//   * Populated by a single passive `ExecutionEvent` subscriber spawned in
//     `bus::v2_subscribers::spawn_event_ref_cache_sub` — the subscriber
//     never mutates DB / fires further events; it just acks and inserts.
//   * Bounded retention: a soft cap evicts the oldest insertion order
//     entries when the cap is hit. `EVENT_REF_CACHE_CAP` is intentionally
//     small (1024) — the cache exists for the "evidence write happens
//     immediately after the publish" case, not for cold history replay.
//   * Lookup miss → `EventRef::unavailable("not in resolver cache")`.
//     We deliberately do NOT block / poll the cache — primary dispatch
//     and evidence write must never wait on a resolver lookup.
//
// Log-query path (the `EventRefStatus::Log` constructor) is reserved for
// a future caller that wants to query `LogReadable::read_from(...)` for
// recent execution events. Today only the in-memory cache is wired so
// every cache hit surfaces as `EventRefStatus::Log` (resolved post-hoc,
// not from the publish call site that ran the dispatch).
// ═════════════════════════════════════════════════════════════════════════

/// Soft cap on the resolver's in-memory cache. When the cache reaches this
/// size, the oldest insertion-order entry is evicted on every new insert.
/// Sized for the "evidence write happens within seconds of the publish"
/// pattern — not a long-term audit store.
pub(crate) const EVENT_REF_CACHE_CAP: usize = 1024;

/// Reason string stamped on `EventRef::unavailable` returned from a cache
/// miss. Kept as a constant so call sites + tests can pin the wire form.
pub(crate) const EVENT_REF_RESOLVER_MISS_REASON: &str =
    "event ref not in resolver cache";

/// Reason stamped when neither the cache nor the persistent event log can
/// produce a matching `PlanNodeStateChanged` ref. Surfaces on the
/// `unavailable_reason` field so audit consumers can distinguish a clean
/// "no event ever recorded" miss from a transient query error
/// ([`EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX`] below).
pub(crate) const EVENT_REF_LOG_QUERY_MISS_REASON: &str =
    "event ref not in resolver cache or event log";

/// Prefix stamped when the log-query path itself errored (DB unavailable,
/// pg feature off, etc.). The full reason format is
/// `<prefix>: <underlying error>` so consumers can pivot on the prefix
/// without parsing the inner detail.
pub(crate) const EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX: &str =
    "event ref log-query error";

/// Bound on the number of execution-domain rows the resolver will scan when
/// looking for a `PlanNodeStateChanged` match. The scan is read-only and
/// happens off the dispatch hot path (only when the cache misses) — sized
/// so a few hundred recent transitions are always reachable while a long
/// history replay can never balloon a single lookup. Adjust upward only if
/// the dispatch fan-out grows past this within the cache miss window.
pub(crate) const EVENT_REF_LOG_QUERY_SCAN_LIMIT: usize = 512;

/// Cached event entry — what the subscriber stored alongside the
/// correlation key. Currently just the (source, kind, event_id) triple
/// surfaced as `EventRef`. Kept as a separate struct so the cache value
/// can grow (e.g. add `recorded_at`) without churning the `EventRef`
/// constructor surface.
#[derive(Debug, Clone)]
struct CachedEvent {
    source: String,
    kind: String,
    event_id: String,
}

/// Lightweight in-memory cache + lookup surface for `EventRef` recovery.
///
/// Construct one per daemon and share via `Arc`. The subscriber inserts
/// via [`record_plan_node_state_change`]; consumer call sites query via
/// [`lookup_plan_node_state_change`].
///
/// The resolver NEVER fails the caller: a cache miss returns
/// `EventRef::unavailable(EVENT_REF_RESOLVER_MISS_REASON)` so the
/// downstream evidence write proceeds without a hard error. This matches
/// the wave-16 / task 07 brief: "event lookup failure must not fail
/// primary dispatch / evidence write".
#[derive(Debug)]
pub(crate) struct EventRefResolver {
    inner: Mutex<EventRefResolverInner>,
}

#[derive(Debug)]
struct EventRefResolverInner {
    /// Insertion-order keys for FIFO eviction when cap is hit. We use a
    /// `Vec<String>` rather than `VecDeque<String>` because the eviction
    /// rate is far lower than the lookup rate; the constant-time `pop`
    /// from the back is preferable but eviction happens at the front so
    /// we accept the O(n) shift — `EVENT_REF_CACHE_CAP=1024` keeps the
    /// shift cost negligible against the publish rate.
    order: Vec<String>,
    map: HashMap<String, CachedEvent>,
    cap: usize,
}

impl EventRefResolver {
    /// Construct a resolver with the default capacity ([`EVENT_REF_CACHE_CAP`]).
    pub(crate) fn new() -> Self {
        Self::with_capacity(EVENT_REF_CACHE_CAP)
    }

    /// Construct a resolver with a caller-chosen capacity. Exposed so
    /// tests can pin small caps and exercise the eviction path.
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            inner: Mutex::new(EventRefResolverInner {
                order: Vec::with_capacity(cap.min(64)),
                map: HashMap::with_capacity(cap.min(64)),
                cap,
            }),
        }
    }

    /// Build the deterministic correlation key for a plan-node lifecycle
    /// transition. Mirrors the format `plan_dag::deterministic_plan_node_event_id`
    /// uses for the deterministic-fallback id so cache keys and event ids
    /// are derivable from the same correlation tuple.
    pub(crate) fn plan_node_state_change_key(
        plan_id: &str,
        node_id: &str,
        attempt: u32,
        from: &str,
        to: &str,
    ) -> String {
        format!(
            "plan-node:{}:{}:{}:{}-{}",
            plan_id, node_id, attempt, from, to
        )
    }

    /// Insert a plan-node lifecycle event into the cache. Idempotent on
    /// repeat inserts of the same key (later inserts overwrite the prior
    /// value but do NOT bump the eviction order — the first publish wins
    /// for FIFO eviction purposes).
    ///
    /// `event_id` should be the live `Seq` (or deterministic fallback id)
    /// the publish path used. `source` / `kind` mirror what the producer
    /// would have stamped on `EventRef::new`.
    pub(crate) fn record_plan_node_state_change(
        &self,
        plan_id: &str,
        node_id: &str,
        attempt: u32,
        from: &str,
        to: &str,
        source: impl Into<String>,
        kind: impl Into<String>,
        event_id: impl Into<String>,
    ) {
        let key = Self::plan_node_state_change_key(plan_id, node_id, attempt, from, to);
        let entry = CachedEvent {
            source: source.into(),
            kind: kind.into(),
            event_id: event_id.into(),
        };
        let mut g = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !g.map.contains_key(&key) {
            // FIFO eviction when cap reached.
            if g.order.len() >= g.cap {
                if let Some(oldest) = g.order.first().cloned() {
                    g.order.remove(0);
                    g.map.remove(&oldest);
                }
            }
            g.order.push(key.clone());
        }
        g.map.insert(key, entry);
    }

    /// Look up a plan-node lifecycle event by deterministic correlation
    /// key. On hit returns an `EventRef` with `status=Log` (the id was
    /// recovered post-hoc rather than carried directly from the publish
    /// path). On miss returns `EventRef::unavailable` with the canonical
    /// miss reason — never errors / blocks.
    pub(crate) fn lookup_plan_node_state_change(
        &self,
        plan_id: &str,
        node_id: &str,
        attempt: u32,
        from: &str,
        to: &str,
    ) -> EventRef {
        let key = Self::plan_node_state_change_key(plan_id, node_id, attempt, from, to);
        let g = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        match g.map.get(&key) {
            Some(c) => EventRef::from_log(c.source.clone(), c.kind.clone(), c.event_id.clone()),
            None => EventRef::unavailable(EVENT_REF_RESOLVER_MISS_REASON),
        }
    }

    /// Three-tier lookup: live in-memory cache, then a bounded read-only
    /// scan of the persistent event log (`Domain::Execution` rows newer
    /// than `Seq(0)`, capped at [`EVENT_REF_LOG_QUERY_SCAN_LIMIT`]),
    /// then `EventRef::unavailable(...)` with a descriptive reason.
    ///
    /// Wave-17 / task 06: the persistent path lets evidence refs survive
    /// daemon restarts — once a `PlanNodeStateChanged` was committed to
    /// the event log, any later evidence write can recover it through
    /// this method even though the in-memory cache (`EventRefResolver`)
    /// was dropped on restart.
    ///
    /// Contract:
    ///   * Cache hit returns immediately with `EventRef::from_log` (no
    ///     log scan triggered).
    ///   * Cache miss triggers `LogReadable::read_from(Domain::Execution,
    ///     Seq(0), EVENT_REF_LOG_QUERY_SCAN_LIMIT)`. We scan the result
    ///     for `PlanNodeStateChanged` payloads whose deterministic
    ///     correlation tuple matches the requested key. First match wins
    ///     (rows are ordered by `seq` ASC; matching is exact on
    ///     plan_id/node_id/from/to and `attempt` if the row carried one).
    ///   * Log query failure returns `EventRef::unavailable` with reason
    ///     `<EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX>: <error>` — the
    ///     primary dispatch must NEVER fail because of this.
    ///   * No match returns `EventRef::unavailable(EVENT_REF_LOG_QUERY_MISS_REASON)`.
    ///
    /// `attempt` is matched when the persisted row carries a non-empty
    /// `attempt` field; rows without an `attempt` are accepted as
    /// matches for any caller `attempt` (mirrors the `attempt: Option<u32>`
    /// shape on `ExecutionEvent::PlanNodeStateChanged` — early producers
    /// may not populate it).
    pub(crate) async fn lookup_or_query_plan_node_state_change(
        &self,
        log: &dyn LogReadable,
        plan_id: &str,
        node_id: &str,
        attempt: u32,
        from: &str,
        to: &str,
    ) -> EventRef {
        // Tier 1 — live in-memory cache.
        let cached = self.lookup_plan_node_state_change(plan_id, node_id, attempt, from, to);
        if cached.status == EventRefStatus::Log {
            return cached;
        }
        // Tier 2 — bounded read-only scan of the persistent log.
        match log
            .read_from(Domain::Execution, Seq(0), EVENT_REF_LOG_QUERY_SCAN_LIMIT)
            .await
        {
            Ok(rows) => {
                for row in rows.into_iter().rev() {
                    // Iterate newest-first so the latest matching row wins
                    // when the same correlation tuple was published more
                    // than once (e.g. attempt re-emit after a partial
                    // failure).
                    if row.kind != "plan_node_state_changed" {
                        continue;
                    }
                    let ev: ExecutionEvent =
                        match serde_json::from_value(row.payload.clone()) {
                            Ok(e) => e,
                            Err(_) => continue,
                        };
                    if let ExecutionEvent::PlanNodeStateChanged {
                        plan_id: rp,
                        node_id: rn,
                        from: rf,
                        to: rt,
                        attempt: ra,
                        ..
                    } = ev
                    {
                        if rp != plan_id || rn != node_id || rf != from || rt != to {
                            continue;
                        }
                        if let Some(ra) = ra {
                            if ra != attempt {
                                continue;
                            }
                        }
                        // Match. Populate the cache so subsequent lookups
                        // skip the scan.
                        let seq_id = row.seq.0.to_string();
                        self.record_plan_node_state_change(
                            plan_id,
                            node_id,
                            attempt,
                            from,
                            to,
                            "execution",
                            "plan_node_state_changed",
                            seq_id.clone(),
                        );
                        return EventRef::from_log(
                            "execution",
                            "plan_node_state_changed",
                            seq_id,
                        );
                    }
                }
                // Tier 3 — no match in the log either.
                EventRef::unavailable(EVENT_REF_LOG_QUERY_MISS_REASON)
            }
            Err(err) => EventRef::unavailable(format!(
                "{}: {}",
                EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX, err
            )),
        }
    }

    /// Current entry count — exposed for tests / metrics.
    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        match self.inner.lock() {
            Ok(g) => g.map.len(),
            Err(poisoned) => poisoned.into_inner().map.len(),
        }
    }
}

impl Default for EventRefResolver {
    fn default() -> Self {
        Self::new()
    }
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
                "p", node, 1, "ready", "running", "execution", "plan_node_state_changed", seq,
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
                "p", "n1", 1, "ready", "running", "execution", "plan_node_state_changed", seq,
            );
        }
        // n1 still occupies one slot; one more slot is free.
        resolver.record_plan_node_state_change(
            "p", "n2", 1, "ready", "running", "execution", "plan_node_state_changed", "X",
        );
        // Both entries must survive.
        let r1 = resolver.lookup_plan_node_state_change("p", "n1", 1, "ready", "running");
        assert_eq!(r1.event_id.as_deref(), Some("2"), "overwrite kept newest value");
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
        ) -> Result<
            Vec<missiond_core::event::log::LoggedEvent>,
            missiond_core::event::log::LogError,
        > {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
            .lookup_or_query_plan_node_state_change(
                &log,
                "p",
                "n1",
                1,
                "ready",
                "running",
            )
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
        assert_eq!(v["event_ref_source"], "execution");
        assert!(
            v.get("event_ref_warning").is_none(),
            "live ref with no warning omits the warning key"
        );
    }

    #[test]
    fn primary_event_ref_surface_emits_status_source_for_log() {
        let r = EventRef::from_log("execution", "plan_node_state_changed", "77");
        let v = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, kind::DISPATCH)
            .with_primary_event_ref(&r, Some("recovered from event log".to_string()))
            .add_execution_event(r)
            .into_json();
        assert_eq!(v["event_ref_status"], "log");
        assert_eq!(v["event_ref_source"], "execution");
        assert_eq!(v["event_ref_warning"], "recovered from event log");
    }

    #[test]
    fn primary_event_ref_surface_lifts_unavailable_reason_when_no_explicit_warning() {
        let r = EventRef::unavailable(EVENT_REF_LOG_QUERY_MISS_REASON);
        let v = EvidenceEntry::new(source::PLAN_DAG_NODE_DISPATCH, kind::DISPATCH)
            .with_primary_event_ref(&r, None)
            .add_execution_event(r)
            .into_json();
        assert_eq!(v["event_ref_status"], "unavailable");
        assert!(
            v.get("event_ref_source").is_none(),
            "unavailable ref carries no source"
        );
        assert_eq!(
            v["event_ref_warning"],
            EVENT_REF_LOG_QUERY_MISS_REASON,
            "unavailable_reason is lifted as the warning when no explicit warning is set"
        );
    }

    #[test]
    fn resolver_lookup_failure_degrades_to_unavailable_with_reason() {
        // Pinning the no-throw contract from the wave-16/07 brief: a
        // missing key MUST return `EventRef::unavailable(...)` rather than
        // panic / Err — the resolver never fails the caller.
        let resolver = EventRefResolver::new();
        let r =
            resolver.lookup_plan_node_state_change("plan", "node", 1, "ready", "succeeded");
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
