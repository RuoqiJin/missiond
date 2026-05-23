use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::Value;

use super::super::acceptance::{AcceptanceMode, AcceptanceRequires};
use super::super::rollback::{RollbackCascadeMode, RollbackPolicy};
#[cfg(test)]
use super::scanner::parse_plan_dag;
use super::types::{
    DagBuildError, DagNode, ParsedDag, FAILURE_POLICY_CONTINUE, FAILURE_POLICY_FAIL_FAST,
    VALID_TARGETS,
};

/// Parse and validate a PLAN.lisp body, returning a topologically-sorted node
/// list ready for sequential dispatch.
#[cfg(test)]
pub(in crate::handlers::knowledge::plan_dag) fn build_validated_dag(
    sexp: &str,
) -> std::result::Result<(ParsedDag, Vec<String>), DagBuildError> {
    let parsed = parse_plan_dag(sexp);
    validate_parsed_dag(parsed)
}

pub(in crate::handlers::knowledge::plan_dag) fn build_validated_dag_from_contract_json(
    contract: &Value,
) -> std::result::Result<(ParsedDag, Vec<String>), DagBuildError> {
    let parsed = parsed_dag_from_contract_json(contract)?;
    validate_parsed_dag(parsed)
}

fn validate_parsed_dag(
    parsed: ParsedDag,
) -> std::result::Result<(ParsedDag, Vec<String>), DagBuildError> {
    if parsed.nodes.is_empty() {
        return Err(DagBuildError::NoNodes);
    }

    // Unique id check.
    let mut seen: HashSet<&str> = HashSet::new();
    for n in &parsed.nodes {
        if !seen.insert(n.id.as_str()) {
            return Err(DagBuildError::DuplicateId(n.id.clone()));
        }
    }

    // wave-16 / task 05 — fail fast on any retry hint that did not
    // parse cleanly. Raised BEFORE target / dependency checks so the
    // author sees the most actionable error first; a typo'd
    // `:retry-count "thrice"` is a contract bug, not a topology bug.
    for n in &parsed.nodes {
        if let Some((key, raw, detail)) = &n.retry_parse_error {
            return Err(DagBuildError::InvalidRetryHint {
                node_id: n.id.clone(),
                key: key.clone(),
                raw: raw.clone(),
                detail: detail.clone(),
            });
        }
    }

    // Target whitelist + self-dep + missing dep.
    let id_set: HashSet<&str> = parsed.nodes.iter().map(|n| n.id.as_str()).collect();
    for n in &parsed.nodes {
        if !VALID_TARGETS.contains(&n.target.as_str()) {
            return Err(DagBuildError::InvalidTarget {
                node_id: n.id.clone(),
                target: n.target.clone(),
            });
        }
        for dep in &n.depends_on {
            if dep == &n.id {
                return Err(DagBuildError::SelfDependency(n.id.clone()));
            }
            if !id_set.contains(dep.as_str()) {
                return Err(DagBuildError::DependencyMissing {
                    node_id: n.id.clone(),
                    missing: dep.clone(),
                });
            }
        }
    }

    let order = kahn_topo_sort(&parsed.nodes)?;

    // wave-18 / task 03 — cross-node acceptance fan-in validation.
    // Runs AFTER topo sort so we can compute transitive ancestors via
    // the existing dependency graph. The four checks (in order):
    //
    //   1. Every entry in `:acceptance-depends-on` must be a declared
    //      node id.
    //   2. If `:acceptance-depends-on` is non-empty, `:acceptance-requires`
    //      must be a recognised mode.
    //   3. Under `evidence_keys` mode, `:acceptance-source-node` must
    //      be set AND must appear in `:acceptance-depends-on` (and
    //      therefore in the plan; the depends-on check handles plan
    //      membership).
    //   4. Every entry in `:acceptance-depends-on` must be a (transitive)
    //      `:depends-on` ancestor of the current node — otherwise the
    //      source node's evidence may not yet exist when this node's
    //      acceptance phase runs (we deliberately do NOT promote
    //      acceptance deps to execution deps; that would silently
    //      change dispatch order).
    let ancestors = compute_transitive_ancestors(&parsed.nodes);
    for n in &parsed.nodes {
        if n.acceptance_depends_on.is_empty()
            && n.acceptance_requires_raw
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && n.acceptance_source_node
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            continue;
        }
        // (1) plan membership
        for dep in &n.acceptance_depends_on {
            if !id_set.contains(dep.as_str()) {
                return Err(DagBuildError::AcceptanceDependencyMissing {
                    node_id: n.id.clone(),
                    missing: dep.clone(),
                });
            }
        }
        // (2) requires mode
        if !n.acceptance_depends_on.is_empty() {
            if n.acceptance_requires_kind().is_none() {
                return Err(DagBuildError::AcceptanceFanInRequiresMissing {
                    node_id: n.id.clone(),
                    raw: n.acceptance_requires_raw.clone(),
                });
            }
        } else if n.acceptance_source_node.is_some()
            || n.acceptance_requires_raw
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        {
            // Author wrote :acceptance-requires / :acceptance-source-node
            // without :acceptance-depends-on. Treat as a fan-in declaration
            // missing its dependency list — still fail-fast so the typo
            // surfaces at build time instead of silently doing nothing.
            return Err(DagBuildError::AcceptanceFanInRequiresMissing {
                node_id: n.id.clone(),
                raw: Some("fan-in declared without :acceptance-depends-on".to_string()),
            });
        }
        // (3) source node (only meaningful under evidence_keys; we
        //     surface a structured error if it's set under a wrong
        //     mode so the typo doesn't go silent).
        if let Some(src_raw) = n.acceptance_source_node.as_deref() {
            let src = src_raw.trim();
            if src.is_empty() {
                return Err(DagBuildError::AcceptanceSourceNodeInvalid {
                    node_id: n.id.clone(),
                    detail: "value is empty".to_string(),
                });
            }
            if !id_set.contains(src) {
                return Err(DagBuildError::AcceptanceSourceNodeInvalid {
                    node_id: n.id.clone(),
                    detail: format!("`{}` is not declared in this plan", src),
                });
            }
            if !n.acceptance_depends_on.iter().any(|d| d == src) {
                return Err(DagBuildError::AcceptanceSourceNodeInvalid {
                    node_id: n.id.clone(),
                    detail: format!(
                        "`{}` must also appear in this node's `:acceptance-depends-on`",
                        src
                    ),
                });
            }
        }
        if matches!(
            n.acceptance_requires_kind(),
            Some(AcceptanceRequires::EvidenceKeys)
        ) && n.acceptance_source_node.is_none()
        {
            return Err(DagBuildError::AcceptanceSourceNodeInvalid {
                node_id: n.id.clone(),
                detail: ":acceptance-requires \"evidence_keys\" requires `:acceptance-source-node`"
                    .to_string(),
            });
        }
        // (4) every fan-in dep must be a transitive :depends-on ancestor
        if let Some(set) = ancestors.get(n.id.as_str()) {
            for dep in &n.acceptance_depends_on {
                if !set.contains(dep.as_str()) {
                    return Err(DagBuildError::AcceptanceFanInDepNotAncestor {
                        node_id: n.id.clone(),
                        ancestor: dep.clone(),
                    });
                }
            }
        }
    }

    // wave-19 / task 10 — forward `:compensate-node` validation. The
    // forward declaration lives on the failing (cascade-root) node and
    // points AT a compensation node id. Three checks (in order):
    //
    //   (a) value MUST be non-empty after trimming;
    //   (b) value MUST resolve to a declared node id AND MUST NOT name
    //       the failing node itself (self-reference is rejected);
    //   (c) when the named compensation node ALSO carries
    //       `:compensates "<X>"`, then `<X>` MUST equal the failing
    //       node id. Any disagreement fails fast — the scheduler MUST
    //       NOT silently pick one direction over the other (the wave-18
    //       reverse contract is the source of truth for the cascade
    //       evaluator, but accepting a contradicting forward ref would
    //       hide the author's mistake).
    //
    // Forward refs that name a compensation node WITHOUT a reverse
    // `:compensates` declaration are accepted and surface through
    // `compute_compensation_order` as if the compensation node had
    // declared `:compensates "<failing-node-id>"` (forward + reverse
    // are unioned). This is the new feature: authors who prefer
    // top-down readability declare cascade structure on the failing
    // node side without touching the compensation node.
    let by_id: HashMap<&str, &DagNode> = parsed.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    for n in &parsed.nodes {
        let Some(raw) = n.compensate_node.as_deref() else {
            continue;
        };
        let trimmed = raw.trim();
        // (a) non-empty
        if trimmed.is_empty() {
            return Err(DagBuildError::CompensateNodeInvalid {
                node_id: n.id.clone(),
                key: "compensate-node".to_string(),
                raw: raw.to_string(),
                detail: "value is empty".to_string(),
            });
        }
        // (b) self-reference rejected
        if trimmed == n.id {
            return Err(DagBuildError::CompensateNodeInvalid {
                node_id: n.id.clone(),
                key: "compensate-node".to_string(),
                raw: raw.to_string(),
                detail: format!(
                    "names the failing node itself (`{}`); a node cannot be its own \
                     compensation",
                    n.id
                ),
            });
        }
        // (b cont.) plan membership
        let Some(comp) = by_id.get(trimmed) else {
            return Err(DagBuildError::CompensateNodeInvalid {
                node_id: n.id.clone(),
                key: "compensate-node".to_string(),
                raw: raw.to_string(),
                detail: format!("`{}` is not declared in this plan", trimmed),
            });
        };
        // (c) reverse-direction agreement (only when the comp node ALSO
        //     declared `:compensates`). Compared case-insensitively to
        //     mirror the existing `compute_compensation_order` matching.
        if let Some(reverse_raw) = comp.compensates.as_deref() {
            let reverse = reverse_raw.trim();
            if !reverse.is_empty() && reverse.to_ascii_lowercase() != n.id.to_ascii_lowercase() {
                return Err(DagBuildError::CompensateDirectionMismatch {
                    failing_node_id: n.id.clone(),
                    comp_node_id: comp.id.clone(),
                    reverse_target: reverse.to_string(),
                });
            }
        }
    }

    Ok((parsed, order))
}

fn parsed_dag_from_contract_json(contract: &Value) -> Result<ParsedDag, DagBuildError> {
    let schema_version = contract
        .get("schema_version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if schema_version != "missiond.plan-contract.v2" {
        return Err(DagBuildError::InvalidContract(format!(
            "expected schema_version=missiond.plan-contract.v2, got `{}`",
            if schema_version.is_empty() {
                "<missing>"
            } else {
                schema_version
            }
        )));
    }

    let payload = contract
        .get("payload")
        .ok_or_else(|| DagBuildError::InvalidContract("missing payload object".to_string()))?;
    let nodes = payload
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            DagBuildError::InvalidContract("payload.nodes must be an array".to_string())
        })?;

    let mut out = Vec::with_capacity(nodes.len());
    for (idx, node) in nodes.iter().enumerate() {
        let object = node.as_object().ok_or_else(|| {
            DagBuildError::InvalidContract(format!("payload.nodes[{}] must be an object", idx))
        })?;
        let id = required_string(node, "id", idx)?;
        let target = required_string(node, "target", idx)?;
        let mut unsupported_fields = unsupported_fields_from_contract(node, idx)?;
        add_unknown_enum_unsupported(node, "acceptance_mode", &mut unsupported_fields, |raw| {
            AcceptanceMode::parse(raw).is_some()
        });
        add_unknown_enum_unsupported(
            node,
            "acceptance_requires",
            &mut unsupported_fields,
            |raw| AcceptanceRequires::parse(raw).is_some(),
        );
        add_unknown_enum_unsupported(node, "review_gate", &mut unsupported_fields, |raw| {
            let lc = raw.trim().to_ascii_lowercase();
            matches!(
                lc.as_str(),
                "" | "none" | "question-event" | "question_event"
            )
        });
        add_unknown_enum_unsupported(node, "rollback_policy", &mut unsupported_fields, |raw| {
            RollbackPolicy::parse(raw).is_some()
        });
        add_unknown_enum_unsupported(node, "rollback_cascade", &mut unsupported_fields, |raw| {
            RollbackCascadeMode::parse(raw).is_some()
        });

        let failure_policy = optional_string(node, "failure_policy", idx)?
            .unwrap_or_else(|| FAILURE_POLICY_FAIL_FAST.to_string());
        let failure_policy = match failure_policy.as_str() {
            FAILURE_POLICY_FAIL_FAST | FAILURE_POLICY_CONTINUE => failure_policy,
            other => {
                unsupported_fields.push(("failure_policy".to_string(), other.to_string()));
                FAILURE_POLICY_FAIL_FAST.to_string()
            }
        };

        let retry_count = optional_u32(node, "retry_count", idx)?;
        let retry_delay_ms = optional_u64(node, "retry_delay_ms", idx)?;
        let retry_parse_error = retry_parse_error_from_contract(node, idx)?;

        let node = DagNode {
            id,
            target,
            objective: optional_string(node, "objective", idx)?,
            depends_on: string_vec(node, "depends_on", idx)?,
            condition: optional_string(node, "condition", idx)?,
            failure_policy,
            timeout_ms: optional_i64(node, "timeout_ms", idx)?,
            dispatch_strategy: optional_string(node, "dispatch_strategy", idx)?,
            target_project: optional_string(node, "target_project", idx)?,
            requested_cwd: optional_string(node, "requested_cwd", idx)?,
            flow_id: optional_string(node, "flow_id", idx)?,
            scope: optional_string(node, "scope", idx)?,
            commit_policy: optional_string(node, "commit_policy", idx)?,
            owned_files_raw: raw_string_list(node, "owned_files", idx)?,
            forbidden_files_raw: raw_string_list(node, "forbidden_files", idx)?,
            acceptance_commands_raw: raw_string_list(node, "acceptance_commands", idx)?,
            acceptance_mode_raw: optional_string(node, "acceptance_mode", idx)?,
            acceptance_evidence_keys_raw: raw_string_list(node, "acceptance_evidence_keys", idx)?,
            acceptance_depends_on: string_vec(node, "acceptance_depends_on", idx)?,
            acceptance_requires_raw: optional_string(node, "acceptance_requires", idx)?,
            acceptance_source_node: optional_string(node, "acceptance_source_node", idx)?,
            workstation_dispatch_flag: optional_string(node, "workstation_dispatch", idx)?,
            review_gate: optional_string(node, "review_gate", idx)?,
            review_action: optional_string(node, "review_action", idx)?,
            review_text: optional_string(node, "review_text", idx)?,
            retry_count,
            retry_delay_ms,
            retry_parse_error,
            rollback_policy: optional_string(node, "rollback_policy", idx)?,
            rollback_objective: optional_string(node, "rollback_objective", idx)?,
            rollback_owned_files_raw: raw_string_list(node, "rollback_owned_files", idx)?,
            rollback_acceptance_commands_raw: raw_string_list(
                node,
                "rollback_acceptance_commands",
                idx,
            )?,
            compensates: optional_string(node, "compensates", idx)?,
            compensate_node: optional_string(node, "compensate_node", idx)?,
            rollback_cascade: optional_string(node, "rollback_cascade", idx)?,
            rollback_after: string_vec(node, "rollback_after", idx)?,
            unsupported_fields,
        };
        if object.is_empty() {
            return Err(DagBuildError::InvalidContract(format!(
                "payload.nodes[{}] must not be empty",
                idx
            )));
        }
        out.push(node);
    }

    Ok(ParsedDag {
        nodes: out,
        unsupported_top_forms: Vec::new(),
    })
}

fn required_string(node: &Value, key: &str, idx: usize) -> Result<String, DagBuildError> {
    optional_string(node, key, idx)?
        .and_then(non_empty)
        .ok_or_else(|| {
            DagBuildError::InvalidContract(format!(
                "payload.nodes[{}].{} must be a non-empty string",
                idx, key
            ))
        })
}

fn optional_string(node: &Value, key: &str, idx: usize) -> Result<Option<String>, DagBuildError> {
    match node.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => Ok(non_empty(raw.clone())),
        Some(Value::Bool(value)) => Ok(Some(value.to_string())),
        Some(Value::Number(value)) => Ok(Some(value.to_string())),
        Some(_) => Err(DagBuildError::InvalidContract(format!(
            "payload.nodes[{}].{} must be a scalar or null",
            idx, key
        ))),
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn string_vec(node: &Value, key: &str, idx: usize) -> Result<Vec<String>, DagBuildError> {
    match node.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| match value {
                Value::String(raw) => Ok(raw.trim().to_string()),
                Value::Bool(value) => Ok(value.to_string()),
                Value::Number(value) => Ok(value.to_string()),
                _ => Err(DagBuildError::InvalidContract(format!(
                    "payload.nodes[{}].{} must be an array of scalars",
                    idx, key
                ))),
            })
            .filter_map(|result| match result {
                Ok(value) if value.is_empty() => None,
                other => Some(other),
            })
            .collect(),
        Some(_) => Err(DagBuildError::InvalidContract(format!(
            "payload.nodes[{}].{} must be an array",
            idx, key
        ))),
    }
}

fn raw_string_list(node: &Value, key: &str, idx: usize) -> Result<Option<String>, DagBuildError> {
    let values = string_vec(node, key, idx)?;
    if values.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lisp_string_list(&values)))
    }
}

fn lisp_string_list(values: &[String]) -> String {
    let escaped: Vec<String> = values
        .iter()
        .map(|value| format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", escaped.join(" "))
}

fn optional_i64(node: &Value, key: &str, idx: usize) -> Result<Option<i64>, DagBuildError> {
    match node.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_i64().map(Some).ok_or_else(|| {
            DagBuildError::InvalidContract(format!(
                "payload.nodes[{}].{} must be an integer",
                idx, key
            ))
        }),
        Some(Value::String(raw)) if raw.trim().is_empty() => Ok(None),
        Some(Value::String(raw)) => raw.trim().parse::<i64>().map(Some).map_err(|err| {
            DagBuildError::InvalidContract(format!(
                "payload.nodes[{}].{} must be an integer: {}",
                idx, key, err
            ))
        }),
        Some(_) => Err(DagBuildError::InvalidContract(format!(
            "payload.nodes[{}].{} must be an integer or null",
            idx, key
        ))),
    }
}

fn optional_u32(node: &Value, key: &str, idx: usize) -> Result<Option<u32>, DagBuildError> {
    match optional_i64(node, key, idx)? {
        None => Ok(None),
        Some(n) if n >= 0 => Ok(Some(n.min(u32::MAX as i64) as u32)),
        Some(_) => Err(DagBuildError::InvalidContract(format!(
            "payload.nodes[{}].{} must be non-negative",
            idx, key
        ))),
    }
}

fn optional_u64(node: &Value, key: &str, idx: usize) -> Result<Option<u64>, DagBuildError> {
    match optional_i64(node, key, idx)? {
        None => Ok(None),
        Some(n) if n >= 0 => Ok(Some(n as u64)),
        Some(_) => Err(DagBuildError::InvalidContract(format!(
            "payload.nodes[{}].{} must be non-negative",
            idx, key
        ))),
    }
}

fn retry_parse_error_from_contract(
    node: &Value,
    idx: usize,
) -> Result<Option<(String, String, String)>, DagBuildError> {
    let Some(value) = node.get("retry_parse_error") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let object = value.as_object().ok_or_else(|| {
        DagBuildError::InvalidContract(format!(
            "payload.nodes[{}].retry_parse_error must be an object or null",
            idx
        ))
    })?;
    let key = object
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("retry-count")
        .to_string();
    let raw = object
        .get("raw")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let detail = object
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("invalid retry hint")
        .to_string();
    Ok(Some((key, raw, detail)))
}

fn unsupported_fields_from_contract(
    node: &Value,
    idx: usize,
) -> Result<Vec<(String, String)>, DagBuildError> {
    let Some(value) = node.get("unsupported_fields") else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = value.as_array().ok_or_else(|| {
        DagBuildError::InvalidContract(format!(
            "payload.nodes[{}].unsupported_fields must be an array",
            idx
        ))
    })?;
    values
        .iter()
        .enumerate()
        .map(|(field_idx, value)| {
            let object = value.as_object().ok_or_else(|| {
                DagBuildError::InvalidContract(format!(
                    "payload.nodes[{}].unsupported_fields[{}] must be an object",
                    idx, field_idx
                ))
            })?;
            let key = object
                .get("key")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    DagBuildError::InvalidContract(format!(
                        "payload.nodes[{}].unsupported_fields[{}].key must be a string",
                        idx, field_idx
                    ))
                })?
                .trim()
                .trim_start_matches(':')
                .to_string();
            let value = object
                .get("value")
                .map(contract_value_to_string)
                .unwrap_or_default();
            Ok((key, value))
        })
        .collect()
}

fn contract_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(raw) => raw.clone(),
        Value::Bool(raw) => raw.to_string(),
        Value::Number(raw) => raw.to_string(),
        Value::Array(values) => {
            let parts: Vec<String> = values.iter().map(contract_value_to_string).collect();
            lisp_string_list(&parts)
        }
        Value::Object(_) => value.to_string(),
    }
}

fn add_unknown_enum_unsupported(
    node: &Value,
    key: &str,
    unsupported_fields: &mut Vec<(String, String)>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(raw) = node.get(key).and_then(plan_contract_scalar_to_string) else {
        return;
    };
    if raw.trim().is_empty() || valid(raw.trim()) {
        return;
    }
    if !unsupported_fields
        .iter()
        .any(|(existing_key, existing_value)| existing_key == key && existing_value == raw.trim())
    {
        unsupported_fields.push((key.to_string(), raw.trim().to_string()));
    }
}

fn plan_contract_scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) => Some(raw.clone()),
        Value::Bool(raw) => Some(raw.to_string()),
        Value::Number(raw) => Some(raw.to_string()),
        _ => None,
    }
}

/// wave-18 / task 03 — compute the set of transitive `:depends-on`
/// ancestors for every node, keyed by node id. Pure helper; runs once
/// per `build_validated_dag` call so the cross-node acceptance fan-in
/// validator can verify each `:acceptance-depends-on` entry already
/// sits upstream in the execution-ordering DAG.
fn compute_transitive_ancestors(nodes: &[DagNode]) -> HashMap<String, HashSet<String>> {
    let by_id: HashMap<&str, &DagNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for n in nodes {
        let mut acc: HashSet<String> = HashSet::new();
        let mut stack: Vec<String> = n.depends_on.clone();
        while let Some(id) = stack.pop() {
            if !acc.insert(id.clone()) {
                continue;
            }
            if let Some(parent) = by_id.get(id.as_str()) {
                for p in &parent.depends_on {
                    if !acc.contains(p) {
                        stack.push(p.clone());
                    }
                }
            }
        }
        out.insert(n.id.clone(), acc);
    }
    out
}

/// Kahn's topological sort. Stable across runs because we sort the per-tier
/// ready set by node id (tests rely on this for deterministic output).
fn kahn_topo_sort(nodes: &[DagNode]) -> std::result::Result<Vec<String>, DagBuildError> {
    let mut indeg: HashMap<&str, usize> = HashMap::new();
    let mut succ: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in nodes {
        indeg.entry(n.id.as_str()).or_insert(0);
        succ.entry(n.id.as_str()).or_default();
    }
    for n in nodes {
        for dep in &n.depends_on {
            *indeg.entry(n.id.as_str()).or_insert(0) += 1;
            succ.entry(dep.as_str()).or_default().push(n.id.as_str());
        }
    }
    // Use a sorted ready-set so output is deterministic.
    let mut ready: BTreeSet<&str> = indeg
        .iter()
        .filter_map(|(k, v)| if *v == 0 { Some(*k) } else { None })
        .collect();
    let mut order: Vec<String> = Vec::new();
    while let Some(&head) = ready.iter().next() {
        ready.remove(head);
        order.push(head.to_string());
        if let Some(succs) = succ.get(head) {
            for &s in succs {
                let entry = indeg.get_mut(s).expect("succ exists");
                *entry -= 1;
                if *entry == 0 {
                    ready.insert(s);
                }
            }
        }
    }
    if order.len() != nodes.len() {
        // Surface the node ids still carrying non-zero in-degree so the error
        // message points at the offending cycle members.
        let mut leftover: Vec<String> = indeg
            .iter()
            .filter_map(|(k, v)| if *v > 0 { Some(k.to_string()) } else { None })
            .collect();
        leftover.sort();
        return Err(DagBuildError::Cycle(leftover));
    }
    Ok(order)
}
