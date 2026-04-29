use missiond_mcp::tools::{error_codes, ToolContent, ToolError, ToolResult};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Schema label embedded in every emitted task_runner block. Mirrors
/// the wave28-02 CLI's `PLAN_SCHEMA` so downstream consumers can
/// verify the wire shape against the same constant.
pub(super) const SCHEMA: &str = "missiond.task-runner-plan.v0";
pub(super) const MANIFEST_SCHEMA: &str = "missiond.task-runner-manifest.v1";

/// Recognised top-level `task_runner_mode` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskRunnerMode {
    /// Default. The task_runner block is NOT emitted; the response
    /// is byte-identical to the wave-15..27 baseline.
    Off,
    /// Read the manifest, project deterministic facts, and emit
    /// `applied=false`. Never alters dispatch.
    DryRun,
}

/// Parse the optional `task_runner_mode` arg. Returns `Off` when the
/// arg is absent / null / the literal string `"off"`. Returns a
/// structured `INVALID_PARAM` error for any other value (including
/// `apply` / `auto` / unknown strings / non-strings) so a typo cannot
/// silently route the surface through an unimplemented mode.
pub(super) fn parse_task_runner_mode(args: &Value) -> Result<TaskRunnerMode, ToolResult> {
    let raw = match args.get("task_runner_mode") {
        None | Some(Value::Null) => return Ok(TaskRunnerMode::Off),
        Some(v) => v,
    };
    let s = match raw.as_str() {
        Some(s) => s.trim(),
        None => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    "task_runner_mode must be a string",
                )
                .with_suggestion("expected one of: \"off\", \"dry_run\""),
            ));
        }
    };
    match s {
        "" | "off" => Ok(TaskRunnerMode::Off),
        "dry_run" => Ok(TaskRunnerMode::DryRun),
        other => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "task_runner_mode `{}` is not supported in this surface (wave28-04 only ships `off` and `dry_run`)",
                    other
                ),
            )
            .with_suggestion(
                "expected one of: \"off\" (default; no task_runner block) or \"dry_run\" (informational block, applied=false)",
            ),
        )),
    }
}

/// Splice the task_runner block onto a successful response. No-op
/// when `mode=Off` so callers that never opted in observe the
/// wave-15..27 byte-shape, AND no file I/O is performed even if
/// `task_runner_manifest_path` is supplied. Errors are passed through
/// unchanged — we never decorate a structured error with the block.
pub(super) fn attach_task_runner_block(
    mut result: ToolResult,
    mode: TaskRunnerMode,
    args: &Value,
) -> ToolResult {
    if matches!(mode, TaskRunnerMode::Off) {
        return result;
    }
    if result.is_error.unwrap_or(false) {
        return result;
    }
    let block = compute_runner_block(args);
    let Some(ToolContent::Text { text }) = result.content.first_mut() else {
        return result;
    };
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return result;
    };
    if let Some(map) = value.as_object_mut() {
        // Never overwrite a pre-existing block — preserves any forward-
        // compatible attachment a downstream layer may add.
        map.entry("task_runner".to_string()).or_insert(block);
    }
    *text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.clone());
    result
}

/// Top-level entry called only on the dry_run path. Reads the
/// optional manifest, projects deterministic facts, and assembles
/// the response block. NEVER panics; surfaces I/O / parse failures
/// via the `manifest_status` field + `task_runner_warning` field.
pub(super) fn compute_runner_block(args: &Value) -> Value {
    // Pure projection split: parsing/loading produces a `RunnerInputs`
    // intermediate so the response builder is testable in isolation
    // (mirrors wave27-03's compute_recommendation_block split).
    let inputs = load_runner_inputs(args);
    build_runner_response_block(&inputs)
}

/// Read manifest path arg + load + parse. Always returns a structured
/// `RunnerInputs`; failure modes degrade through `manifest_status`.
fn load_runner_inputs(args: &Value) -> RunnerInputs {
    let manifest_path = arg_string(args, "task_runner_manifest_path");
    let Some(path_str) = manifest_path else {
        return RunnerInputs {
            manifest_path: None,
            manifest_status: ManifestStatus::Missing,
            warning: Some(
                "task_runner_mode=dry_run requires task_runner_manifest_path".to_string(),
            ),
            manifest: None,
        };
    };
    let resolved = resolve_manifest_path(&path_str);
    let raw = match std::fs::read_to_string(&resolved) {
        Ok(s) => s,
        Err(e) => {
            let warning = format!("manifest read failed: {}", e);
            let status = if e.kind() == std::io::ErrorKind::NotFound {
                ManifestStatus::Missing
            } else {
                ManifestStatus::Unreadable
            };
            return RunnerInputs {
                manifest_path: Some(path_str),
                manifest_status: status,
                warning: Some(warning),
                manifest: None,
            };
        }
    };
    match parse_manifest(&raw) {
        Ok(m) => RunnerInputs {
            manifest_path: Some(path_str),
            manifest_status: ManifestStatus::Used,
            warning: None,
            manifest: Some(m),
        },
        Err(msg) => RunnerInputs {
            manifest_path: Some(path_str),
            manifest_status: ManifestStatus::Malformed,
            warning: Some(format!("manifest parse failed: {}", msg)),
            manifest: None,
        },
    }
}

/// Resolve a manifest path verbatim (mirrors `resolve_policy_path`
/// in `router_policy_dry_run`: absolute paths stay absolute;
/// relative paths are passed verbatim and resolved against the
/// daemon's CWD). Free of repo-root detection logic so tests can
/// pass tmp paths.
fn resolve_manifest_path(input: &str) -> PathBuf {
    let p = Path::new(input);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        PathBuf::from(input)
    }
}

fn arg_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Manifest read / parse status surfaced on the response block.
/// Mirrors the wave26-03 `BackendRegistryInfo` enum-as-status pattern
/// and is encoded as a string in the `manifest_status` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManifestStatus {
    Used,
    Missing,
    Unreadable,
    Malformed,
}

impl ManifestStatus {
    fn as_str(self) -> &'static str {
        match self {
            ManifestStatus::Used => "used",
            ManifestStatus::Missing => "missing",
            ManifestStatus::Unreadable => "unreadable",
            ManifestStatus::Malformed => "malformed",
        }
    }
}

#[derive(Debug, Clone)]
struct RunnerInputs {
    manifest_path: Option<String>,
    manifest_status: ManifestStatus,
    warning: Option<String>,
    manifest: Option<Manifest>,
}

/// Minimal Rust subset of the wave28-01 manifest. Only the fields
/// needed for the wave28-04 projection are extracted; unknown keys
/// are tolerated so future schema growth does not require a daemon
/// update.
#[derive(Debug, Clone)]
struct Manifest {
    wave: String,
    productive_only: bool,
    overlap_policy: String,
    nodes: Vec<ManifestNode>,
}

#[derive(Debug, Clone)]
struct ManifestNode {
    task_id: String,
    depends_on: Vec<String>,
    dispatch_group: String,
    verification_tier: String,
    estimated_minutes: u64,
    write_scope: Vec<String>,
}

/// Manifest parser. Returns a structured `Manifest` or a string
/// describing the failure. Reuses the wave24-04 in-module S-expression
/// tokeniser by re-implementing the minimal subset needed here (so
/// this module stays self-contained and the cross-module surface
/// stays small). The parser deliberately validates ONLY the bits
/// required to project the response — full schema validation lives
/// in the wave28-01 Node checker.
fn parse_manifest(input: &str) -> Result<Manifest, String> {
    let tokens = tokenize(input)?;
    let mut cursor = TokenCursor::new(&tokens);
    let form = cursor
        .read_form()
        .ok_or_else(|| "no form found".to_string())?;
    if cursor.peek().is_some() {
        return Err("multiple top-level forms".to_string());
    }
    let list = match form {
        Sexp::List(items) => items,
        _ => return Err("expected (task-runner-manifest ...) at top level".to_string()),
    };
    let mut iter = list.into_iter();
    let head = iter
        .next()
        .ok_or_else(|| "empty top-level list".to_string())?;
    match head {
        Sexp::Atom(s) if s == "task-runner-manifest" => {}
        _ => return Err("expected (task-runner-manifest ...) at top level".to_string()),
    }
    // Skip the manifest id atom (next item).
    let _id = iter.next();

    let mut schema: Option<String> = None;
    let mut wave: Option<String> = None;
    let mut productive_only: Option<bool> = None;
    let mut overlap_policy: Option<String> = None;
    let mut nodes: Vec<ManifestNode> = Vec::new();
    let mut pending_keyword: Option<String> = None;

    for item in iter {
        if let Some(key) = pending_keyword.take() {
            match key.as_str() {
                ":schema" => schema = Some(sexp_as_text(&item)),
                ":wave" => wave = Some(sexp_as_text(&item)),
                ":productive_only" => productive_only = Some(sexp_as_bool(&item)),
                ":overlap_policy" => overlap_policy = Some(sexp_as_text(&item)),
                // Tolerated header keys: :brief_mode, :shared_preamble_path,
                // :description, :generated_at, :generator. Future schema
                // growth does not require a daemon update.
                _ => {}
            }
            continue;
        }
        match &item {
            Sexp::Keyword(k) => pending_keyword = Some(k.clone()),
            Sexp::List(inner) => {
                if matches!(inner.first(), Some(Sexp::Atom(h)) if h == "node") {
                    let entry = parse_node_entry(inner)?;
                    nodes.push(entry);
                }
                // Other top-level lists are tolerated.
            }
            _ => {}
        }
    }

    let schema = schema.ok_or_else(|| "missing :schema header field".to_string())?;
    if schema != MANIFEST_SCHEMA {
        return Err(format!(
            "header :schema `{}` does not match {}",
            schema, MANIFEST_SCHEMA
        ));
    }
    let wave = wave.ok_or_else(|| "missing :wave header field".to_string())?;
    let productive_only =
        productive_only.ok_or_else(|| "missing :productive_only header field".to_string())?;
    // Default per wave28-01 schema: reject. Unknown values are coerced
    // to reject so a typo is loud (severity=error).
    let overlap_policy = overlap_policy
        .map(|s| {
            if s == "warn" || s == "reject" {
                s
            } else {
                "reject".to_string()
            }
        })
        .unwrap_or_else(|| "reject".to_string());

    Ok(Manifest {
        wave,
        productive_only,
        overlap_policy,
        nodes,
    })
}

fn parse_node_entry(items: &[Sexp]) -> Result<ManifestNode, String> {
    // items[0] is the `node` atom.
    let mut task_id: Option<String> = None;
    let mut depends_on: Option<Vec<String>> = None;
    let mut dispatch_group: Option<String> = None;
    let mut verification_tier: Option<String> = None;
    let mut estimated_minutes: Option<u64> = None;
    let mut write_scope: Option<Vec<String>> = None;
    let mut idx = 1usize;
    // Optional first positional after `node` may be the task id atom
    // (mirrors the wave28-01 schema: many manifests use `(node <id> :prop ...)`
    // even though :task_id is the canonical key). Tolerate both shapes.
    if let Some(item) = items.get(idx) {
        if !matches!(item, Sexp::Keyword(_)) {
            let candidate = sexp_as_text(item);
            if !candidate.is_empty() {
                task_id = Some(candidate);
                idx += 1;
            }
        }
    }
    while idx < items.len() {
        let key = match &items[idx] {
            Sexp::Keyword(k) => k.clone(),
            _ => {
                idx += 1;
                continue;
            }
        };
        idx += 1;
        if idx >= items.len() {
            break;
        }
        let value = &items[idx];
        idx += 1;
        match key.as_str() {
            ":task_id" => task_id = Some(sexp_as_text(value)),
            ":depends_on" => depends_on = Some(sexp_as_string_vec(value)),
            ":dispatch_group" => dispatch_group = Some(sexp_as_text(value)),
            ":verification_tier" => verification_tier = Some(sexp_as_text(value)),
            ":estimated_minutes" => estimated_minutes = sexp_as_positive_u64(value),
            ":write_scope" => write_scope = Some(sexp_as_string_vec(value)),
            // Tolerated optional fields: :heartbeat_minutes, :notes, :owner, :kind.
            _ => {}
        }
    }
    let task_id = task_id.ok_or_else(|| "node missing :task_id".to_string())?;
    if task_id.is_empty() {
        return Err("node has empty :task_id".to_string());
    }
    let depends_on = depends_on.unwrap_or_default();
    let dispatch_group =
        dispatch_group.ok_or_else(|| format!("node `{}` missing :dispatch_group", task_id))?;
    if dispatch_group.is_empty() {
        return Err(format!("node `{}` has empty :dispatch_group", task_id));
    }
    let verification_tier = verification_tier
        .ok_or_else(|| format!("node `{}` missing :verification_tier", task_id))?;
    let estimated_minutes = estimated_minutes
        .ok_or_else(|| format!("node `{}` missing or invalid :estimated_minutes", task_id))?;
    let write_scope = write_scope.unwrap_or_default();
    Ok(ManifestNode {
        task_id,
        depends_on,
        dispatch_group,
        verification_tier,
        estimated_minutes,
        write_scope,
    })
}

fn sexp_as_text(value: &Sexp) -> String {
    match value {
        Sexp::Atom(s) => s.clone(),
        Sexp::Str(s) => s.clone(),
        Sexp::Keyword(s) => s.clone(),
        Sexp::List(_) => String::new(),
    }
}

fn sexp_as_bool(value: &Sexp) -> bool {
    match value {
        Sexp::Atom(s) => s == "true",
        Sexp::Str(s) => s == "true",
        _ => false,
    }
}

fn sexp_as_positive_u64(value: &Sexp) -> Option<u64> {
    let raw = match value {
        Sexp::Atom(s) => s.clone(),
        Sexp::Str(s) => s.clone(),
        _ => return None,
    };
    raw.parse::<u64>().ok().filter(|n| *n > 0)
}

fn sexp_as_string_vec(value: &Sexp) -> Vec<String> {
    match value {
        Sexp::List(items) => items
            .iter()
            .map(sexp_as_text)
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Pure response-block builder — operates on the loaded inputs and
/// returns a serialisable JSON object. Splitting load and projection
/// mirrors the wave27-03 `compute_recommendation_block` pattern so
/// this code is testable without touching the filesystem.
fn build_runner_response_block(inputs: &RunnerInputs) -> Value {
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

// ---- tiny tokeniser / cursor (self-contained copy) ----------------
// Mirrors the wave24-04 in-module S-expression tokeniser. Re-implemented
// here so this module stays self-contained; the parsers in the two
// modules accept the same surface but extract different schemas.

#[derive(Debug, Clone)]
enum Sexp {
    Atom(String),
    Str(String),
    Keyword(String),
    List(Vec<Sexp>),
}

#[derive(Debug, Clone)]
enum Token {
    LParen,
    RParen,
    LBracket,
    RBracket,
    Atom(String),
    Str(String),
    Keyword(String),
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == ';' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '(' {
            out.push(Token::LParen);
            i += 1;
            continue;
        }
        if c == ')' {
            out.push(Token::RParen);
            i += 1;
            continue;
        }
        if c == '[' {
            out.push(Token::LBracket);
            i += 1;
            continue;
        }
        if c == ']' {
            out.push(Token::RBracket);
            i += 1;
            continue;
        }
        if c == '"' {
            let mut s = String::new();
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' {
                    i += 1;
                    if i < chars.len() {
                        s.push(chars[i]);
                        i += 1;
                    }
                    continue;
                }
                if ch == '"' {
                    i += 1;
                    break;
                }
                s.push(ch);
                i += 1;
            }
            out.push(Token::Str(s));
            continue;
        }
        if c == ':' {
            let mut s = String::from(":");
            i += 1;
            while i < chars.len() && !is_atom_terminator(chars[i]) {
                s.push(chars[i]);
                i += 1;
            }
            out.push(Token::Keyword(s));
            continue;
        }
        let mut s = String::new();
        while i < chars.len() && !is_atom_terminator(chars[i]) {
            s.push(chars[i]);
            i += 1;
        }
        if !s.is_empty() {
            out.push(Token::Atom(s));
        }
    }
    Ok(out)
}

fn is_atom_terminator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '"' | ';')
}

struct TokenCursor<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> TokenCursor<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }
    fn read_form(&mut self) -> Option<Sexp> {
        let tok = self.tokens.get(self.pos)?;
        match tok {
            Token::LParen => {
                self.pos += 1;
                let mut items = Vec::new();
                while let Some(t) = self.tokens.get(self.pos) {
                    if matches!(t, Token::RParen) {
                        self.pos += 1;
                        return Some(Sexp::List(items));
                    }
                    if let Some(form) = self.read_form() {
                        items.push(form);
                    } else {
                        break;
                    }
                }
                Some(Sexp::List(items))
            }
            Token::LBracket => {
                self.pos += 1;
                let mut items = Vec::new();
                while let Some(t) = self.tokens.get(self.pos) {
                    if matches!(t, Token::RBracket) {
                        self.pos += 1;
                        return Some(Sexp::List(items));
                    }
                    if let Some(form) = self.read_form() {
                        items.push(form);
                    } else {
                        break;
                    }
                }
                Some(Sexp::List(items))
            }
            Token::RParen | Token::RBracket => None,
            Token::Atom(s) => {
                self.pos += 1;
                Some(Sexp::Atom(s.clone()))
            }
            Token::Str(s) => {
                self.pos += 1;
                Some(Sexp::Str(s.clone()))
            }
            Token::Keyword(s) => {
                self.pos += 1;
                Some(Sexp::Keyword(s.clone()))
            }
        }
    }
}
