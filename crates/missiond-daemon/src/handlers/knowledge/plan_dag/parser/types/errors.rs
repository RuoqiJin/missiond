use missiond_mcp::tools::{error_codes, ToolError, ToolResult};

use super::node::VALID_TARGETS;

#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::plan_dag) enum DagBuildError {
    InvalidContract(String),
    NoNodes,
    DuplicateId(String),
    InvalidTarget {
        node_id: String,
        target: String,
    },
    DependencyMissing {
        node_id: String,
        missing: String,
    },
    SelfDependency(String),
    Cycle(Vec<String>),
    /// wave-16 / task 05 — author supplied a retry hint with a value
    /// that fails parsing (negative number, non-numeric, overflow). We
    /// fail fast here instead of silently dropping the value into
    /// `unsupported_fields` because retry counts directly drive the
    /// scheduler's attempt budget — a typo'd `:retry-count "thrice"`
    /// must NOT be interpreted as "no retry", or the author would
    /// silently lose the policy they declared.
    InvalidRetryHint {
        node_id: String,
        key: String,
        raw: String,
        detail: String,
    },
    /// wave-18 / task 03 — `:acceptance-depends-on` references a node id
    /// that is not declared in this plan. Fail-fast so the typo cannot
    /// silently degrade fan-in to "no gate".
    AcceptanceDependencyMissing {
        node_id: String,
        missing: String,
    },
    /// wave-18 / task 03 — `:acceptance-source-node` either references a
    /// node id that is not declared in this plan OR was omitted while
    /// `:acceptance-requires "evidence_keys"` was declared.
    AcceptanceSourceNodeInvalid {
        node_id: String,
        detail: String,
    },
    /// wave-18 / task 03 — `:acceptance-depends-on` is non-empty but
    /// `:acceptance-requires` is absent / unrecognised. The fan-in
    /// evaluator cannot decide accept / reject without a recognised
    /// mode, so we fail-fast.
    AcceptanceFanInRequiresMissing {
        node_id: String,
        raw: Option<String>,
    },
    /// wave-18 / task 03 — a node listed in `:acceptance-depends-on`
    /// is NOT (transitively) an ancestor of this node via the existing
    /// `:depends-on` topology. Acceptance dependencies must not silently
    /// introduce new execution-ordering: the source node's evidence
    /// must already exist when this node's acceptance phase runs.
    AcceptanceFanInDepNotAncestor {
        node_id: String,
        ancestor: String,
    },
    /// wave-19 / task 10 — a node declared `:compensate-node "<X>"` (or
    /// `:compensate-ref`) but `X` is invalid: empty value, references
    /// the failing node itself (self-ref), or names a node id not
    /// declared in this plan. Fail-fast so a typo cannot silently
    /// degrade cascade discovery to "no compensation".
    CompensateNodeInvalid {
        node_id: String,
        key: String,
        raw: String,
        detail: String,
    },
    /// wave-19 / task 10 — both directions of the compensate
    /// relationship are declared but they disagree: the forward
    /// `:compensate-node "X"` declared on the failing node `F` points
    /// at compensation node `X`, but `X`'s reverse `:compensates "Y"`
    /// names some `Y != F`. The scheduler MUST NOT silently choose one
    /// direction; the validator fails fast so the author resolves the
    /// disagreement explicitly.
    CompensateDirectionMismatch {
        failing_node_id: String,
        comp_node_id: String,
        reverse_target: String,
    },
}

impl DagBuildError {
    pub(in crate::handlers::knowledge::plan_dag) fn into_tool_result(self) -> ToolResult {
        match self {
            DagBuildError::InvalidContract(detail) => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("DAG plan contract is invalid: {}", detail),
                )
                .with_suggestion(
                    "recompile the plan contract with `missiond-lispc emit-plan-contract`; \
                     production DAG execution only accepts missiond.plan-contract.v2 with typed nodes",
                ),
            ),
            DagBuildError::NoNodes => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    "scheduler_mode=dag_v1 found no `(node :id ... :target ...)` forms in plan.sexp_text",
                )
                .with_suggestion(
                    "DAG v1 only parses explicit (node ...) forms; rewrite the plan to use them \
                     or fall back to the default (single-node) scheduler mode",
                ),
            ),
            DagBuildError::DuplicateId(id) => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("DAG node id `{}` is duplicated; node ids must be unique", id),
                ),
            ),
            DagBuildError::InvalidTarget { node_id, target } => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "DAG node `{}` has unsupported :target `{}`; valid: {:?}",
                        node_id, target, VALID_TARGETS
                    ),
                ),
            ),
            DagBuildError::DependencyMissing { node_id, missing } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` depends on `{}` which is not declared in this plan",
                            node_id, missing
                        ),
                    ),
                )
            }
            DagBuildError::SelfDependency(id) => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("DAG node `{}` declares itself in :depends-on", id),
                ),
            ),
            DagBuildError::InvalidRetryHint { node_id, key, raw, detail } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` has invalid `:{}` value `{}`: {}",
                            node_id, key, raw, detail
                        ),
                    )
                    .with_suggestion(
                        "supply a non-negative integer ≤ 3 for `:retry-count` / `:max-attempts` \
                         (the cap), or remove the hint to keep the default single-attempt contract",
                    ),
                )
            }
            DagBuildError::Cycle(cycle) => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "DAG contains a cycle involving nodes: {}",
                        cycle.join(" -> ")
                    ),
                ),
            ),
            DagBuildError::AcceptanceDependencyMissing { node_id, missing } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` declares `:acceptance-depends-on` referencing `{}` \
                             which is not declared in this plan",
                            node_id, missing
                        ),
                    )
                    .with_suggestion(
                        "every entry in `:acceptance-depends-on` MUST be a node id declared \
                         elsewhere in this plan and MUST also be a (transitive) `:depends-on` \
                         ancestor of the current node",
                    ),
                )
            }
            DagBuildError::AcceptanceSourceNodeInvalid { node_id, detail } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` has invalid `:acceptance-source-node`: {}",
                            node_id, detail
                        ),
                    )
                    .with_suggestion(
                        "set `:acceptance-source-node` to a node id that also appears in this \
                         node's `:acceptance-depends-on` list (only used under \
                         `:acceptance-requires \"evidence_keys\"`)",
                    ),
                )
            }
            DagBuildError::AcceptanceFanInRequiresMissing { node_id, raw } => {
                let detail = match raw {
                    Some(r) if !r.trim().is_empty() => format!(
                        "got `{}`; expected one of: all_succeeded | any_succeeded | evidence_keys",
                        r
                    ),
                    _ => "field is missing".to_string(),
                };
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` declares `:acceptance-depends-on` but \
                             `:acceptance-requires` {}",
                            node_id, detail
                        ),
                    )
                    .with_suggestion(
                        "add `:acceptance-requires \"all_succeeded\"` (or `any_succeeded` / \
                         `evidence_keys`) to specify how the fan-in gate decides; remove \
                         `:acceptance-depends-on` if no fan-in is intended",
                    ),
                )
            }
            DagBuildError::AcceptanceFanInDepNotAncestor { node_id, ancestor } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` declares `:acceptance-depends-on` referencing `{}` \
                             which is not a (transitive) `:depends-on` ancestor of `{}`",
                            node_id, ancestor, node_id
                        ),
                    )
                    .with_suggestion(
                        "the source node's evidence must already exist when this node's \
                         acceptance phase runs — add the source to this node's `:depends-on` \
                         (directly or via an existing chain) so the scheduler dispatches them \
                         in the correct order",
                    ),
                )
            }
            DagBuildError::CompensateNodeInvalid { node_id, key, raw, detail } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` has invalid `:{}` value `{}`: {}",
                            node_id, key, raw, detail
                        ),
                    )
                    .with_suggestion(
                        "set `:compensate-node` (or `:compensate-ref`) to the id of a \
                         compensation node declared elsewhere in this plan; the value MUST \
                         NOT name the failing node itself, and the named compensation node's \
                         own `:compensates` (when present) MUST point back at the failing node",
                    ),
                )
            }
            DagBuildError::CompensateDirectionMismatch {
                failing_node_id,
                comp_node_id,
                reverse_target,
            } => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "DAG node `{}` declares `:compensate-node \"{}\"` but `{}` declares \
                         `:compensates \"{}\"` — forward and reverse compensate directions \
                         disagree; the scheduler refuses to silently pick one",
                        failing_node_id, comp_node_id, comp_node_id, reverse_target
                    ),
                )
                .with_suggestion(
                    "make the two directions agree: either change `:compensate-node` on the \
                     failing node, or change `:compensates` on the compensation node so they \
                     name each other (forward + reverse must be symmetric)",
                ),
            ),
        }
    }
}
