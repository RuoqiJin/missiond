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
//!     sidecar JSON at
//!     `<project_root>/.missiond/v3/runtime/plans/<plan_id>.evidence.json`
//!     remains the single source of truth. Legacy `.missiond/v2/plans`
//!     sidecars stay readable/updatable as compatibility fallbacks.
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

mod append;
mod entry;
mod event_ref;
mod legacy;
mod resolver;
mod taxonomy;

#[allow(unused_imports)]
pub(crate) use append::*;
#[allow(unused_imports)]
pub(crate) use entry::*;
#[allow(unused_imports)]
pub(crate) use event_ref::*;
#[allow(unused_imports)]
pub(crate) use legacy::*;
#[allow(unused_imports)]
pub(crate) use resolver::{
    EventRefResolver, EVENT_REF_CACHE_CAP, EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX,
    EVENT_REF_LOG_QUERY_MISS_REASON, EVENT_REF_LOG_QUERY_SCAN_LIMIT,
    EVENT_REF_RESOLVER_MISS_REASON,
};
#[allow(unused_imports)]
pub(crate) use taxonomy::*;

#[cfg(test)]
mod tests;
