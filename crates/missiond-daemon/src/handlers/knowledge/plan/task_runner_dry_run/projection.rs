use super::manifest::{Manifest, ManifestNode, RunnerInputs};
use super::SCHEMA;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Pure response-block builder — operates on the loaded inputs and
/// returns a serialisable JSON object. Splitting load and projection
/// mirrors the wave27-03 `compute_recommendation_block` pattern so
/// this code is testable without touching the filesystem.
pub(super) fn build_runner_response_block(inputs: &RunnerInputs) -> Value {
    let mut block = Map::new();
    block.insert("schema".to_string(), Value::String(SCHEMA.to_string()));
    block.insert(
        "manifest_status".to_string(),
        Value::String(inputs.manifest_status.as_str().to_string()),
    );
    // applied is hard-coded literal — NEVER computed (cross-wave
    // invariant pinned since wave24-04). Always emitted as a
    // `Value::Bool(false)` so a future change cannot silently flip
    // it via string coercion.
    block.insert("applied".to_string(), Value::Bool(false));
    if let Some(p) = inputs.manifest_path.clone() {
        block.insert("manifest_path".to_string(), Value::String(p));
    }
    if let Some(w) = inputs.warning.clone() {
        block.insert("task_runner_warning".to_string(), Value::String(w));
    }
    // Manifest-derived projection. When the manifest failed to load
    // we still emit the deterministic empty shape so consumers can
    // rely on the field set.
    match &inputs.manifest {
        Some(manifest) => {
            let projection = project_manifest(manifest);
            block.insert("wave".to_string(), Value::String(projection.wave));
            block.insert(
                "productive_only".to_string(),
                Value::Bool(projection.productive_only),
            );
            block.insert(
                "batches".to_string(),
                Value::Array(
                    projection
                        .batches
                        .into_iter()
                        .map(|b| Value::Array(b.into_iter().map(Value::String).collect::<Vec<_>>()))
                        .collect(),
                ),
            );
            block.insert(
                "critical_path_minutes".to_string(),
                json!(projection.critical_path_minutes),
            );
            block.insert(
                "total_estimated_minutes".to_string(),
                json!(projection.total_estimated_minutes),
            );
            block.insert(
                "verification_tier_counts".to_string(),
                json!({
                    "local": projection.verification_tier_counts.local,
                    "smoke": projection.verification_tier_counts.smoke,
                    "full": projection.verification_tier_counts.full,
                }),
            );
            block.insert(
                "overlap_diagnostics".to_string(),
                Value::Array(
                    projection
                        .overlap_diagnostics
                        .into_iter()
                        .map(|d| {
                            json!({
                                "pair": d.pair,
                                "paths": d.paths,
                                "severity": d.severity,
                                "group": d.group,
                            })
                        })
                        .collect(),
                ),
            );
        }
        None => {
            // Degraded mode — no manifest. Surface a minimal
            // deterministic empty projection so downstream consumers
            // can tell "no nodes" apart from "no shape".
            block.insert("wave".to_string(), Value::String(String::new()));
            block.insert("productive_only".to_string(), Value::Bool(false));
            block.insert("batches".to_string(), Value::Array(Vec::new()));
            block.insert("critical_path_minutes".to_string(), json!(0));
            block.insert("total_estimated_minutes".to_string(), json!(0));
            block.insert(
                "verification_tier_counts".to_string(),
                json!({"local": 0, "smoke": 0, "full": 0}),
            );
            block.insert("overlap_diagnostics".to_string(), Value::Array(Vec::new()));
        }
    }
    Value::Object(block)
}

#[derive(Debug, Default)]
struct VerificationTierCounts {
    local: u32,
    smoke: u32,
    full: u32,
}

#[derive(Debug, Clone)]
struct OverlapDiagnostic {
    pair: Vec<String>,
    paths: Vec<String>,
    severity: String,
    group: String,
}

#[derive(Debug)]
struct ManifestProjection {
    wave: String,
    productive_only: bool,
    batches: Vec<Vec<String>>,
    critical_path_minutes: u64,
    total_estimated_minutes: u64,
    verification_tier_counts: VerificationTierCounts,
    overlap_diagnostics: Vec<OverlapDiagnostic>,
}

/// Pure projection of a parsed manifest into the response shape.
/// Mirrors `scripts/plan-task-runner.mjs`'s `planFromManifestObject`
/// for the projection facts (excluding the on-disk task-contract
/// join, which the daemon deliberately skips — that lives in the
/// wave28-05 batch verifier).
fn project_manifest(manifest: &Manifest) -> ManifestProjection {
    // Stable input ordering: sort nodes by task_id so the projection
    // is deterministic regardless of source order.
    let mut sorted_nodes: Vec<&ManifestNode> = manifest.nodes.iter().collect();
    sorted_nodes.sort_by(|a, b| a.task_id.cmp(&b.task_id));

    let id_to_node: HashMap<&str, &ManifestNode> = sorted_nodes
        .iter()
        .map(|n| (n.task_id.as_str(), *n))
        .collect();

    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    for n in &sorted_nodes {
        in_degree.insert(n.task_id.clone(), 0);
        dependents.insert(n.task_id.clone(), Vec::new());
    }
    for n in &sorted_nodes {
        for dep in &n.depends_on {
            if !id_to_node.contains_key(dep.as_str()) {
                continue;
            }
            *in_degree.get_mut(&n.task_id).unwrap() += 1;
            dependents.get_mut(dep).unwrap().push(n.task_id.clone());
        }
    }

    // Topological batching with dispatch_group boundaries (mirrors
    // wave28-02's planFromManifestObject loop). On cycle / drop, the
    // daemon emits an empty batches list rather than panicking; the
    // consumer is expected to consult the wave28-02 CLI for full
    // diagnostics.
    let mut batches: Vec<Vec<String>> = Vec::new();
    let mut remaining: BTreeSet<String> = sorted_nodes.iter().map(|n| n.task_id.clone()).collect();
    let mut safety = sorted_nodes.len() + 2;
    while !remaining.is_empty() && safety > 0 {
        safety -= 1;
        let ready: Vec<String> = remaining
            .iter()
            .filter(|id| in_degree.get(id.as_str()).copied().unwrap_or(0) == 0)
            .cloned()
            .collect();
        if ready.is_empty() {
            // Cycle — bail out without infinite-looping. Empty
            // batches signals the degraded state to the consumer.
            batches.clear();
            break;
        }
        // Group ready set by dispatch_group; lexicographic group
        // ordering for determinism (sort stability of strings).
        let mut by_group: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in &ready {
            let g = id_to_node[id.as_str()].dispatch_group.clone();
            by_group.entry(g).or_default().push(id.clone());
        }
        for (_g, mut batch) in by_group {
            batch.sort();
            for id in &batch {
                remaining.remove(id);
                let deps = dependents.get(id).cloned().unwrap_or_default();
                for d in deps {
                    if let Some(v) = in_degree.get_mut(&d) {
                        if *v > 0 {
                            *v -= 1;
                        }
                    }
                }
            }
            batches.push(batch);
        }
    }

    // Critical-path: longest dependency chain by estimated_minutes.
    let mut longest: HashMap<String, u64> = HashMap::new();
    let mut critical_path: u64 = 0;
    for n in &sorted_nodes {
        let v = longest_from(n.task_id.as_str(), &id_to_node, &dependents, &mut longest);
        if v > critical_path {
            critical_path = v;
        }
    }

    let mut total_minutes: u64 = 0;
    for n in &sorted_nodes {
        total_minutes = total_minutes.saturating_add(n.estimated_minutes);
    }

    // Verification-tier counts — pre-seeded so a tier with zero nodes
    // still appears in the output (mirrors the Node CLI's deterministic
    // shape).
    let mut tier_counts = VerificationTierCounts::default();
    for n in &sorted_nodes {
        match n.verification_tier.as_str() {
            "local" => tier_counts.local += 1,
            "smoke" => tier_counts.smoke += 1,
            "full" => tier_counts.full += 1,
            _ => {}
        }
    }

    let overlap_diagnostics = collect_overlap_diagnostics(&sorted_nodes, &manifest.overlap_policy);

    ManifestProjection {
        wave: manifest.wave.clone(),
        productive_only: manifest.productive_only,
        batches,
        critical_path_minutes: critical_path,
        total_estimated_minutes: total_minutes,
        verification_tier_counts: tier_counts,
        overlap_diagnostics,
    }
}

fn longest_from(
    id: &str,
    id_to_node: &HashMap<&str, &ManifestNode>,
    dependents: &HashMap<String, Vec<String>>,
    memo: &mut HashMap<String, u64>,
) -> u64 {
    if let Some(v) = memo.get(id) {
        return *v;
    }
    let node = match id_to_node.get(id) {
        Some(n) => *n,
        None => return 0,
    };
    let mut extra: u64 = 0;
    if let Some(children) = dependents.get(id) {
        for c in children {
            let v = longest_from(c.as_str(), id_to_node, dependents, memo);
            if v > extra {
                extra = v;
            }
        }
    }
    let total = node.estimated_minutes.saturating_add(extra);
    memo.insert(id.to_string(), total);
    total
}

fn collect_overlap_diagnostics(
    nodes: &[&ManifestNode],
    overlap_policy: &str,
) -> Vec<OverlapDiagnostic> {
    let severity = if overlap_policy == "warn" {
        "warning"
    } else {
        "error"
    };
    // group -> path -> set(task_ids)
    let mut groups: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for n in nodes {
        if n.dispatch_group.is_empty() {
            continue;
        }
        let by_path = groups.entry(n.dispatch_group.clone()).or_default();
        for entry in &n.write_scope {
            by_path
                .entry(entry.clone())
                .or_default()
                .insert(n.task_id.clone());
        }
    }
    // Pair-of-task_ids, accumulate paths.
    // key = (group, a, b); value = set of paths
    let mut rows: BTreeMap<(String, String, String), BTreeSet<String>> = BTreeMap::new();
    for (group, by_path) in &groups {
        for (path, ids) in by_path {
            if ids.len() < 2 {
                continue;
            }
            let sorted_ids: Vec<&String> = ids.iter().collect();
            for i in 0..sorted_ids.len() {
                for j in (i + 1)..sorted_ids.len() {
                    let a = sorted_ids[i].clone();
                    let b = sorted_ids[j].clone();
                    rows.entry((group.clone(), a, b))
                        .or_default()
                        .insert(path.clone());
                }
            }
        }
    }
    let mut diagnostics: Vec<OverlapDiagnostic> = Vec::new();
    for ((group, a, b), paths) in rows {
        let mut paths_sorted: Vec<String> = paths.into_iter().collect();
        paths_sorted.sort();
        diagnostics.push(OverlapDiagnostic {
            pair: vec![a, b],
            paths: paths_sorted,
            severity: severity.to_string(),
            group,
        });
    }
    // BTreeMap iteration order already sorted by (group, a, b).
    diagnostics
}
