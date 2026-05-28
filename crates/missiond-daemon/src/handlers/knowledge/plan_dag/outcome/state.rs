/// Terminal node state recorded in `NodeResult`. Mirrors the v1 enum so the
/// per-node JSON shape (`state` discriminant + `failed_dep` extra) stays
/// byte-identical for downstream readers; v2 added `SkippedFailFastAbort`
/// to distinguish "we never dispatched you because an unrelated upstream
/// failed under fail-fast" from "your direct dependency failed", and
/// wave-16 / task 04 added `Paused` for the per-node `:review-gate
/// "question-event"` state. `Paused` is the first non-terminal state that
/// surfaces in the per-node JSON — the resume listener (wave-16 / task 02
/// territory) is expected to revive the node in a follow-up dispatch.
#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::plan_dag) enum NodeState {
    Succeeded,
    Failed {
        reason: String,
    },
    SkippedUpstreamFailed {
        failed_dep: String,
    },
    SkippedCondition,
    /// `failure-policy=fail-fast` aborted the scheduler before this node was
    /// ever ready. Distinct from `SkippedUpstreamFailed` because the failing
    /// upstream is not necessarily a transitive dependency — under fail-fast
    /// every still-pending node is force-skipped once the abort flag flips.
    SkippedFailFastAbort {
        aborter: String,
    },
    /// wave-16 / task 04 — node carried `:review-gate "question-event"`,
    /// the scheduler emitted (or attempted to emit) `QuestionEvent::Created`
    /// with [`question_id`] and STOPPED at this node instead of dispatching
    /// the target tool. `bus_publish_warning` carries the warning string
    /// when the publish call errored — the node still pauses (a failed
    /// gate is a real gate; downstream cannot advance) but the response
    /// surfaces the degraded observability path so callers can retry.
    Paused {
        question_id: String,
        bus_publish_warning: Option<String>,
    },
}

/// Per-node lifecycle phase. Drives the wave-scheduler bookkeeping; mapped to
/// `state` discriminants in the response only after the node terminates. The
/// intermediate phases (`Pending`, `Ready`, `Claimed`, `Running`) never leak
/// into the response — they live entirely in the scheduler's internal state
/// map.
///
/// `Ready` is the brief moment between the scheduler computing the ready set
/// and dispatching it to the JoinSet. The current loop transitions
/// `Pending -> Claimed -> Running` (wave-17 / task 02 added the explicit
/// `Claimed` step between ready-set selection and JoinSet hand-off so the
/// claim/lease registry can stamp metadata before the inner handler runs).
/// The variant `Ready` is kept in the enum to satisfy the wave-13/02 spec
/// lifecycle list and to leave room for a future scheduler that materialises
/// a persistent ready queue (`#[allow(dead_code)]` is intentional for now).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::plan_dag) enum NodeLifecycle {
    Pending,
    #[allow(dead_code)]
    Ready,
    /// wave-17 / task 02 — node has had its canonical work_leases claim
    /// registered, but the inner handler has not yet been invoked. Mostly
    /// invisible from the outside: the dispatch path moves through `Claimed`
    /// for one wave-loop cycle before flipping to `Running`. Surfaces on the
    /// `pending -> claimed` evidence row + bus event so observers can pivot
    /// on the new transition without reconstructing it from `pending ->
    /// running` reasoning.
    Claimed,
    Running,
    Succeeded,
    Failed,
    Skipped,
    /// wave-16 / task 04 — node opted into a `:review-gate "question-event"`
    /// gate and the scheduler emitted `QuestionEvent::Created` instead of
    /// dispatching the target tool. Treated as a non-terminal "stop"
    /// state by the wave loop: the scheduler does NOT retry it within
    /// the same call (auto-resume is wave-16 / task 02 territory), and
    /// the node's downstream stays pending until a follow-up resume.
    Paused,
}
