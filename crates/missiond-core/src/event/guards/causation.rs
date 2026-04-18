//! Causation-loop guard — rejects appends whose causation chain has grown
//! too deep.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.4 causation-loop-guard:
//!
//! > Every append's `causation_depth` = triggering event's `causation_depth` + 1.
//! > Max depth = 10. Exceeding it produces `AppendError::CausalLoop` and an
//! > `IncidentEvent`.
//!
//! The bus enforces this at the ingress (both PG and in-memory log writers)
//! — consumers that derive events from other events are responsible for
//! carrying the depth forward in `AppendOpts::causation_depth`.
//!
//! Rationale (frozen lisp §4.4): real causal chains in practice rarely go
//! beyond 5 levels. Ten gives ops margin while still catching bugs that
//! would otherwise self-replicate events forever.

use super::super::log::{AppendError, AppendOpts};

/// Max allowed depth of a causation chain. Frozen lisp §4.4.
///
/// The value is duplicated from `crate::event::log::MAX_CAUSATION_DEPTH` so
/// that callers who only need the guard don't have to import the log
/// module.
pub const MAX_CAUSATION_DEPTH: u8 = 10;

/// Validate that the incoming append's causation depth is within bounds.
///
/// This is called on the append hot path; both [`crate::event::log::LogWriter`]
/// (PG) and [`crate::event::in_memory::InMemoryLog`] invoke it.
///
/// Returns `Err(AppendError::CausalLoop)` when the depth strictly exceeds
/// [`MAX_CAUSATION_DEPTH`]. Equal is allowed — depth 10 is the ceiling,
/// depth 11 is the rejection point. Frozen lisp says "causation_depth ≤ 10".
#[inline]
pub fn check_causation(opts: &AppendOpts) -> Result<(), AppendError> {
    if opts.causation_depth > MAX_CAUSATION_DEPTH {
        return Err(AppendError::CausalLoop {
            depth: opts.causation_depth,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_within_limit_passes() {
        for depth in 0..=MAX_CAUSATION_DEPTH {
            let opts = AppendOpts {
                causation_depth: depth,
                producer_id: "p".into(),
                ..Default::default()
            };
            assert!(check_causation(&opts).is_ok(), "depth {depth} should pass");
        }
    }

    #[test]
    fn depth_equal_to_max_passes() {
        let opts = AppendOpts {
            causation_depth: MAX_CAUSATION_DEPTH,
            producer_id: "p".into(),
            ..Default::default()
        };
        assert!(check_causation(&opts).is_ok());
    }

    #[test]
    fn depth_above_max_rejects_with_causal_loop() {
        let opts = AppendOpts {
            causation_depth: MAX_CAUSATION_DEPTH + 1,
            producer_id: "p".into(),
            ..Default::default()
        };
        match check_causation(&opts) {
            Err(AppendError::CausalLoop { depth }) => {
                assert_eq!(depth, MAX_CAUSATION_DEPTH + 1);
            }
            other => panic!("expected CausalLoop, got {other:?}"),
        }
    }

    #[test]
    fn max_matches_log_module_const() {
        // Keep the two constants in sync — they come from the same frozen
        // lisp contract and drifting them apart would split the guard.
        assert_eq!(MAX_CAUSATION_DEPTH, crate::event::log::MAX_CAUSATION_DEPTH);
    }

    #[test]
    fn saturated_depth_still_rejected() {
        // u8::MAX is > 10, so anything way beyond the cap must also be
        // caught — no silent wrap.
        let opts = AppendOpts {
            causation_depth: u8::MAX,
            producer_id: "p".into(),
            ..Default::default()
        };
        assert!(matches!(
            check_causation(&opts),
            Err(AppendError::CausalLoop { .. })
        ));
    }
}
