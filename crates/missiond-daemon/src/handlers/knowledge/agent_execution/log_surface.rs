use crate::state::AppState;
use missiond_core::event::events::ExecutionEvent;
use tracing::warn;

use super::{parse_kv_pairs, LogFile};

/// Canonical workstation-dispatch strategies surfaced by intent-tools.lisp ::
/// implemented-surface mission_execution :: :workstation-dispatch-record. Kept
/// in sync with `plan.rs::VALID_DISPATCH_STRATEGIES`; unknown / empty inputs
/// normalize to `DEFAULT_DISPATCH_STRATEGY` so legacy callers keep working.
const VALID_DISPATCH_STRATEGIES: &[&str] = &[
    "resident-lisp",
    "fresh-code-alignment",
    "agent-team",
    "mixed",
    "prompt-fallback",
    "unknown",
];
pub(super) const DEFAULT_DISPATCH_STRATEGY: &str = "unknown";

/// Normalize an optional dispatch strategy string against the canonical set.
/// Unknown / empty values fall back to `DEFAULT_DISPATCH_STRATEGY` (`"unknown"`)
/// without erroring; we never hard-fail open() on a strategy mismatch because
/// upstream dispatchers may legitimately surface novel labels we then audit.
pub(super) fn normalize_dispatch_strategy(raw: Option<&str>) -> &'static str {
    let v = raw.unwrap_or("").trim();
    if v.is_empty() {
        return DEFAULT_DISPATCH_STRATEGY;
    }
    for &known in VALID_DISPATCH_STRATEGIES {
        if known == v {
            return known;
        }
    }
    DEFAULT_DISPATCH_STRATEGY
}

/// Forward an `ExecutionEvent` to the v2 bus and log (but never propagate)
/// publish failures. Companion-log writes are already durable on disk; the
/// bus event is a live projection.
pub(super) async fn emit_execution_event(state: &AppState, ev: ExecutionEvent) {
    if let Err(e) = state.bus.publish_execution(ev).await {
        warn!(error = %e, "failed to publish ExecutionEvent (companion log already durable)");
    }
}

/// Build an `ExecutionEvent::Opened` payload from the inputs `action_open`
/// has already validated and normalized. Centralizing the construction
/// keeps the dispatch-metadata mapping (intent-worker.lisp ::
/// claudecode-workstation-orchestration :: execution-strategy-record)
/// in one testable place — the runtime caller and the unit tests stay in
/// lock-step on which open args land in which event slot.
///
/// `dispatch_strategy` always resolves to a canonical string via
/// `normalize_dispatch_strategy`. We surface it on the event verbatim so
/// downstream auditors observe the same label that lives in the companion
/// log meta block. `target_project` / `requested_cwd` are forwarded only
/// when the open args carry them — `Option::is_none` skip-serialize keeps
/// the wire form byte-identical to the legacy 5-field shape otherwise.
pub(super) fn build_opened_event(
    execution_id: &str,
    parent_design: &str,
    scope: &str,
    owner: &str,
    path: String,
    dispatch_strategy: &str,
    target_project: Option<&str>,
    requested_cwd: Option<&str>,
) -> ExecutionEvent {
    ExecutionEvent::Opened {
        execution_id: execution_id.to_string(),
        parent_design: parent_design.to_string(),
        scope: scope.to_string(),
        owner: owner.to_string(),
        path,
        dispatch_strategy: Some(dispatch_strategy.to_string()),
        target_project: target_project.map(|s| s.to_string()),
        requested_cwd: requested_cwd.map(|s| s.to_string()),
    }
}

/// Single tuple of the workstation-dispatch trio surfaced on every
/// `ExecutionEvent` variant that carries dispatch context. Sourced from the
/// companion-log meta block so consumers don't have to re-load the file to
/// correlate the event against its dispatch strategy / target project /
/// requested cwd. All three fields are `None` when the meta block omits the
/// corresponding `:key`, which lets the legacy companion logs (pre-wave12-01)
/// emit cleanly with the default skip-serialize wire form.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct DispatchMeta {
    pub(super) dispatch_strategy: Option<String>,
    pub(super) target_project: Option<String>,
    pub(super) requested_cwd: Option<String>,
}

/// Read the workstation-dispatch trio (`:dispatch-strategy` /
/// `:target-project` / `:requested-cwd`) from the companion-log meta block.
///
/// Mirrors the parsing path used by `action_list` so the live event stream
/// and the dashboard list view see identical strings. Quoted-string atoms
/// have their outer quotes stripped via `trim_matches('"')` to match the
/// downstream contract; whitespace-only values collapse to `None` so a
/// caller that wrote `:target-project ""` doesn't surface a confusing empty
/// label on the bus.
///
/// Returns `DispatchMeta::default()` when the file has no meta block — the
/// caller emits the event without metadata in that case, matching what
/// legacy producers serialized before the trio was added.
pub(super) fn read_dispatch_metadata_from_log(file: &LogFile) -> DispatchMeta {
    let Some(block) = file.find_block("meta") else {
        return DispatchMeta::default();
    };
    let meta = parse_kv_pairs(&file.src, block.children());
    let read = |key: &str| -> Option<String> {
        meta.get(key)
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
    };
    DispatchMeta {
        dispatch_strategy: read("dispatch-strategy"),
        target_project: read("target-project"),
        requested_cwd: read("requested-cwd"),
    }
}
