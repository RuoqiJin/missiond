use super::*;

/// Wire status for the deterministic proposal-hash check on the
/// auto-spawn gate. Mirrors wave-22 / task 03's `ProposalHashStatus`
/// shape so dashboards see a uniform vocabulary across the two gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationProposalHashStatus {
    /// Caller did not supply `workstation_proposal_hash`. Under
    /// `auto_spawn=true` this collapses to a structured error
    /// (`AUTO_SPAWN_MISSING_PROPOSAL_HASH`) BEFORE the gate runs;
    /// surfaced here for completeness when the gate's preflight is
    /// bypassed (unit tests).
    NotSupplied,
    /// Caller-supplied hash matches the bundle's deterministic hash.
    Matches,
    /// Caller-supplied hash does NOT match. Surfaced as a structured
    /// error (`AUTO_SPAWN_PROPOSAL_HASH_MISMATCH`) BEFORE the gate runs.
    Mismatch,
    /// No bundle / no proposals available — hash check is moot.
    NoProposalAvailable,
}

impl WorkstationProposalHashStatus {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            WorkstationProposalHashStatus::NotSupplied => "not_supplied",
            WorkstationProposalHashStatus::Matches => "matches",
            WorkstationProposalHashStatus::Mismatch => "mismatch",
            WorkstationProposalHashStatus::NoProposalAvailable => "no_proposal_available",
        }
    }
}

/// Pure deterministic SHA-256 hash over the LOAD-BEARING fields of a
/// workstation proposal bundle. Truncated to the leading 32 hex chars
/// (128 bits — way more than enough collision resistance for an
/// audit-trail correlator). Mirrors `compute_proposal_hash` (wave-22 /
/// task 03) for symmetry.
///
/// Inputs (canonical form):
///   * literal `"v1"` schema sentinel,
///   * bundle status wire,
///   * each proposal in its received order:
///       `field|value|confidence|safety_status`,
///     joined by `;`.
///
/// The hash is what the caller is expected to echo back via the
/// `workstation_proposal_hash` arg under `auto_spawn=true`. Caller can
/// derive it themselves from the `workstation_proposals` block — we
/// surface the same value under `workstation_proposal_hash` on the
/// `workstation_auto_spawn_gate` block so dashboards can
/// `assert hash == derive(...)` directly.
pub(crate) fn compute_workstation_proposal_hash(bundle: &WorkstationProposalBundle) -> String {
    use sha2::{Digest, Sha256};
    let proposals: Vec<String> = bundle
        .proposals
        .iter()
        .map(|p| {
            let value_str = p
                .value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| p.value.to_string());
            format!(
                "{}|{}|{}|{}",
                p.field,
                value_str,
                p.confidence.as_wire(),
                p.safety_status.as_wire(),
            )
        })
        .collect();
    let payload = format!("v1|{}|{}", bundle.status.as_wire(), proposals.join(";"));
    let mut h = Sha256::new();
    h.update(payload.as_bytes());
    let full = format!("{:x}", h.finalize());
    full.chars().take(32).collect()
}
