use serde_json::{json, Value};
use std::collections::HashMap;

use super::scopes_overlap_pure;
use super::DagNode;

// ───────────────────────────────────────────────────────────────────────
// wave-17 / task 02 — claim / lease discipline for PLAN DAG nodes.
//
// We bind to the existing wave12-01 `mission_execution(action=claim/release)`
// coordination model: same `scopes_overlap` predicate (re-exported from
// `agent_execution::scopes_overlap_pure`), same lease semantics. There is
// NO new lock service here — the registry below is per-`execute_with_concurrency`
// scratch state used to (a) detect overlapping claims between sibling nodes
// in the same DAG run and (b) carry claim metadata through to the per-node
// evidence + response.
//
// Three knobs are surfaced on the `mission_plan(action=execute,
// scheduler_mode=dag_v1)` envelope:
//   * `claim_lease_secs` — default 1800 (30 min), clamped to
//     `[CLAIM_LEASE_SECS_MIN, CLAIM_LEASE_SECS_MAX]` so an authoring
//     mistake cannot stall the scheduler with a 0-second lease nor pin a
//     scope for hours under a typo'd `999999`. Mirrors the wave12-01 ceiling.
//   * `claimer_name` — default `plan-dag-scheduler`. Matches the wave12-01
//     `:claimer` field convention so audit dashboards can pivot on the
//     same identity vocabulary across companion-log claims AND DAG claims.
//   * `enforce_claims` — default `false` for backward compatibility with
//     pre-wave17 callers. When false, claim metadata still surfaces on
//     the evidence + response (so observers can tell the registry tried)
//     but conflicts NEVER block dispatch. When true, an unresolvable
//     overlap fails the node fast with `CLAIM_CONFLICT` and we never
//     hand it to the inner handler.
//
// The `claimed` lifecycle state is added between `Ready` and `Running`
// (wave-13/02 listed it in the spec but the loop transitioned ready ->
// running directly). The transition is now `pending -> claimed -> running`
// for every dispatched node (whether enforce_claims is true or false);
// best-effort claim registration runs on the way to claimed, and the
// registered claim is released on the way out of every terminal state
// (succeeded / failed / paused / skipped).
// ───────────────────────────────────────────────────────────────────────

/// Default lease seconds when the caller did not pass `claim_lease_secs`.
/// Matches the wave12-01 `mission_execution` constant so the two surfaces
/// surface the same default duration.
pub(super) const PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS: i64 = 1800;

/// Lower bound on `claim_lease_secs`. Mirrors the wave12-01 floor — leases
/// shorter than 60 seconds are almost certainly a typo (a wave can take
/// longer than that to drain) and would create churn in the registry.
pub(super) const PLAN_DAG_CLAIM_LEASE_SECS_MIN: i64 = 60;

/// Upper bound on `claim_lease_secs`. Capped at 4 hours so a typo'd
/// `999999` cannot pin a scope for the rest of the day. Authors who
/// genuinely need longer leases should refresh per node; the registry
/// is per-DAG-run and never persists past the wave loop.
pub(super) const PLAN_DAG_CLAIM_LEASE_SECS_MAX: i64 = 14_400;

/// Default claimer identity stamped onto every plan-DAG claim record
/// when the caller did not pass `claimer_name`. Matches the wave12-01
/// `:claimer` vocabulary so audit dashboards can grep one identity.
pub(super) const PLAN_DAG_DEFAULT_CLAIMER_NAME: &str = "plan-dag-scheduler";

/// Provenance label for the `:scope` derivation used by a claim. Surfaced
/// alongside the claim record so dashboards can tell whether the scheduler
/// claimed `:owned-files`, `:scope`, or the synthetic `plan/<id>/node/<id>`
/// fallback. Stable string vocabulary so tests and dashboards can pin them.
pub(super) const CLAIM_SCOPE_SOURCE_OWNED_FILES: &str = "owned_files";
pub(super) const CLAIM_SCOPE_SOURCE_SCOPE: &str = "scope";
pub(super) const CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK: &str = "plan_node_fallback";

/// Parse `claim_lease_secs` from the call args, defaulting to
/// `PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS` and clamping to the
/// `[MIN, MAX]` range. Mirrors `parse_max_parallel_nodes` in spirit:
/// invalid values are silently normalised rather than rejected because
/// the scheduler treats lease bounds as a safety net, not a contract.
pub(super) fn parse_claim_lease_secs(args: &Value) -> i64 {
    args.get("claim_lease_secs")
        .and_then(|v| v.as_i64())
        .unwrap_or(PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS)
        .clamp(PLAN_DAG_CLAIM_LEASE_SECS_MIN, PLAN_DAG_CLAIM_LEASE_SECS_MAX)
}

/// Parse `claimer_name` from the call args, defaulting to
/// `PLAN_DAG_DEFAULT_CLAIMER_NAME`. Empty / whitespace-only strings fall
/// back to the default so a caller passing an empty form field doesn't
/// poison the audit log with a blank claimer.
pub(super) fn parse_claimer_name(args: &Value) -> String {
    args.get("claimer_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(PLAN_DAG_DEFAULT_CLAIMER_NAME)
        .to_string()
}

/// Parse `enforce_claims` from the call args. Default `false` so
/// pre-wave17 callers keep their byte-compatible dispatch contract. Any
/// non-bool value normalises to `false` (no error) — the enforcement is
/// strict opt-in.
pub(super) fn parse_enforce_claims(args: &Value) -> bool {
    args.get("enforce_claims")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Per-node claim scope derivation. Returns the list of scope tokens the
/// claim covers PLUS the provenance source label. Priority (matches the
/// task brief):
///
///   1. `:owned-files` — when at least one file is declared, every file
///      becomes its own scope token. This lets the registry detect
///      overlap at the file granularity (the same way the wave12-01
///      audit + wave16-06 enforce paths do).
///   2. `:scope` — when `:owned-files` is empty but the author declared
///      a free-form `:scope` string. Used verbatim.
///   3. `plan/<plan_id>/node/<node_id>` synthetic fallback — guarantees
///      every dispatched node carries SOME scope so the registry can
///      record the claim even for ungated nodes.
///
/// The function never returns an empty vector: the fallback case always
/// yields one synthetic token. Pure (no AppState) so unit tests can pin
/// the priority directly.
pub(super) fn derive_node_claim_scopes(
    node: &DagNode,
    plan_id: uuid::Uuid,
) -> (Vec<String>, &'static str) {
    let owned: Vec<String> =
        super::super::plan::split_lisp_string_list(node.owned_files_raw.as_deref())
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect();
    if !owned.is_empty() {
        return (owned, CLAIM_SCOPE_SOURCE_OWNED_FILES);
    }
    if let Some(scope) = node.scope.as_deref() {
        let trimmed = scope.trim();
        if !trimmed.is_empty() {
            return (vec![trimmed.to_string()], CLAIM_SCOPE_SOURCE_SCOPE);
        }
    }
    (
        vec![format!("plan/{}/node/{}", plan_id, node.id)],
        CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK,
    )
}

/// Build the deterministic claim id for a plan-DAG node attempt. Pure
/// helper so tests can pin the format without standing up a registry.
/// Layout: `plan-dag:<plan_id>:<node_id>:<attempt>`. Includes the attempt
/// number so retries (wave-16 / task 05) get a fresh claim id rather
/// than colliding with the previous attempt's record.
pub(super) fn derive_plan_dag_claim_id(plan_id: uuid::Uuid, node_id: &str, attempt: u32) -> String {
    format!("plan-dag:{}:{}:{}", plan_id, node_id, attempt)
}

/// Single in-memory claim record carried by `ClaimRegistry`. Fields
/// mirror the wave12-01 `ClaimRecord` shape so dashboards consuming
/// either surface see the same vocabulary; we add `released_at` as
/// `Option` so a single record can describe both the active and
/// released states without splitting the type.
#[derive(Debug, Clone)]
pub(super) struct PlanDagClaim {
    pub claim_id: String,
    pub claimer: String,
    pub scopes: Vec<String>,
    pub scope_source: &'static str,
    pub acquired_at: chrono::DateTime<chrono::Utc>,
    pub lease_expires_at: chrono::DateTime<chrono::Utc>,
    pub released_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl PlanDagClaim {
    /// ISO-8601 second-precision projection of `acquired_at` (matches
    /// wave12-01's `to_rfc3339_opts(SecondsFormat::Secs, true)` so the
    /// two surfaces print timestamps identically).
    pub(super) fn acquired_at_iso(&self) -> String {
        self.acquired_at
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
    pub(super) fn lease_expires_at_iso(&self) -> String {
        self.lease_expires_at
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
    pub(super) fn released_at_iso(&self) -> Option<String> {
        self.released_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    }
}

/// Outcome of a `ClaimRegistry::try_acquire` call. The `Conflict`
/// variant carries enough context (the conflicting claim's id +
/// claimer + the offending pair of scopes) for the scheduler to
/// surface a structured error in `enforce_claims=true` mode and a
/// best-effort warning in `enforce_claims=false` mode.
#[derive(Debug, Clone)]
pub(super) enum ClaimAcquire {
    Acquired(PlanDagClaim),
    Conflict {
        attempted_claim_id: String,
        attempted_scopes: Vec<String>,
        attempted_scope_source: &'static str,
        conflicting_claim_id: String,
        conflicting_claimer: String,
        conflicting_scope: String,
        offending_scope: String,
    },
}

/// Per-DAG-run claim registry. NOT a global lock service — the registry
/// only exists for the lifetime of one `execute_with_concurrency` call.
/// Conflict detection reuses `scopes_overlap_pure` so the predicate
/// matches wave12-01's `action_claim` overlap test byte-for-byte.
#[derive(Debug, Default)]
pub(super) struct ClaimRegistry {
    claims: HashMap<String, PlanDagClaim>,
}

impl ClaimRegistry {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Try to acquire a claim covering `scopes`. Returns:
    ///   * `Acquired(claim)` if no active claim overlaps any of the
    ///     attempted scopes (or every overlap is on a released claim).
    ///   * `Conflict { ... }` otherwise — carries the offending scope
    ///     pair so the caller can surface a structured error.
    ///
    /// "Active" means `released_at.is_none()` AND `now <
    /// lease_expires_at`. Lease-expired claims are treated as
    /// soft-released (mirrors wave12-01). The scheduler never
    /// explicitly garbage-collects them — the registry is per-DAG-run
    /// and dies with the call.
    pub(super) fn try_acquire(
        &mut self,
        claim_id: String,
        claimer: String,
        scopes: Vec<String>,
        scope_source: &'static str,
        lease_secs: i64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> ClaimAcquire {
        for existing in self.claims.values() {
            if existing.released_at.is_some() {
                continue;
            }
            if existing.lease_expires_at < now {
                continue;
            }
            for new_scope in &scopes {
                for held_scope in &existing.scopes {
                    if scopes_overlap_pure(new_scope, held_scope) {
                        let offending = new_scope.clone();
                        let held = held_scope.clone();
                        let conflicting_id = existing.claim_id.clone();
                        let conflicting_claimer = existing.claimer.clone();
                        return ClaimAcquire::Conflict {
                            attempted_claim_id: claim_id,
                            attempted_scopes: scopes,
                            attempted_scope_source: scope_source,
                            conflicting_claim_id: conflicting_id,
                            conflicting_claimer,
                            conflicting_scope: held,
                            offending_scope: offending,
                        };
                    }
                }
            }
        }
        let claim = PlanDagClaim {
            claim_id: claim_id.clone(),
            claimer,
            scopes,
            scope_source,
            acquired_at: now,
            lease_expires_at: now + chrono::Duration::seconds(lease_secs),
            released_at: None,
        };
        self.claims.insert(claim_id, claim.clone());
        ClaimAcquire::Acquired(claim)
    }

    /// Mark `claim_id` released at `now`. Returns the released record
    /// (with `released_at` populated) so the caller can surface the
    /// timestamp on the per-node evidence. Returns `None` if the id is
    /// unknown — the wave loop never calls release without a prior
    /// successful acquire so this only fires under a registry bug.
    pub(super) fn release(
        &mut self,
        claim_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<PlanDagClaim> {
        let entry = self.claims.get_mut(claim_id)?;
        if entry.released_at.is_none() {
            entry.released_at = Some(now);
        }
        Some(entry.clone())
    }

    /// Total number of recorded claims (active + released). Surfaced on
    /// the response so callers can spot "registry never recorded
    /// anything" without walking every node.
    pub(super) fn len(&self) -> usize {
        self.claims.len()
    }
}

/// Pure projection of the claim plan the scheduler WOULD register given
/// the configured knobs. Used by the dry-run response so callers can
/// preview every node's claim metadata without dispatching anything.
/// The projection mirrors `try_acquire` decisions for the EMPTY
/// registry (no overlap detection across nodes) — real runs may flag
/// conflicts the dry-run cannot foresee. The dry-run is therefore a
/// *what-each-node-would-claim* preview, not an outcome prediction.
pub(super) fn build_planned_claims(
    nodes: &[DagNode],
    order: &[String],
    plan_id: uuid::Uuid,
    claimer: &str,
    lease_secs: i64,
    enforce_claims: bool,
) -> Value {
    let by_id: HashMap<&str, &DagNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out: Vec<Value> = Vec::with_capacity(order.len());
    for id in order {
        let Some(n) = by_id.get(id.as_str()) else {
            continue;
        };
        let (scopes, source) = derive_node_claim_scopes(n, plan_id);
        out.push(json!({
            "node_id": n.id,
            "claim_id": derive_plan_dag_claim_id(plan_id, &n.id, 1),
            "claimer": claimer,
            "scopes": scopes,
            "scope_source": source,
            "lease_secs": lease_secs,
            "enforce_claims": enforce_claims,
        }));
    }
    Value::Array(out)
}
