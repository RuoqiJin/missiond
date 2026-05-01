use super::*;

/// Outcome of an evidence-sidecar append. Either `Written` (with the path +
/// final entry count returned by the underlying writer) or `Failed` (with the
/// underlying error text). Failures are NEVER silently swallowed — callers
/// are expected to surface them on the response payload (mirrors the
/// existing `evidence_error` field on plan-runner / DAG-runner responses).
///
/// `entry_count` is preserved on the `Written` variant for upcoming UI /
/// retrospective surfaces that want to show "this dispatch is the Nth entry
/// in the evidence trail" (today's plan-runner / DAG-runner responses only
/// surface the path). `into_legacy_tuple` discards it because the existing
/// `evidence_path` / `evidence_error` response shape predates per-entry
/// counting.
#[derive(Debug, Clone)]
pub(crate) enum AppendOutcome {
    Written {
        path: PathBuf,
        /// `#[allow(dead_code)]`: read by tests only today — see variant
        /// docstring for the future read-out plan.
        #[allow(dead_code)]
        entry_count: usize,
    },
    Failed {
        error: String,
    },
}

impl AppendOutcome {
    /// Convert to a `(path, error)` tuple matching the legacy plan.rs /
    /// plan_dag.rs response shape. Either field is None if the other applies.
    pub(crate) fn into_legacy_tuple(self) -> (Option<String>, Option<String>) {
        match self {
            AppendOutcome::Written { path, .. } => (Some(path.display().to_string()), None),
            AppendOutcome::Failed { error } => (None, Some(error)),
        }
    }
}

/// Wrapper around the existing `append_plan_evidence_entry` that takes a
/// typed [`EvidenceEntry`] and returns a structured [`AppendOutcome`].
///
/// Callers that already have an `AppState` + plan-resolution signals
/// (`project` / `cwd` / `target_project`) should use this. The wrapper keeps
/// the legacy `(Option<String>, Option<String>)` evidence_path/error shape
/// reachable via `AppendOutcome::into_legacy_tuple` for drop-in adoption.
pub(crate) async fn append(
    state: &AppState,
    plan_id: uuid::Uuid,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    entry: EvidenceEntry,
) -> AppendOutcome {
    let payload = entry.into_json();
    match super::super::plan::append_plan_evidence_entry(
        state,
        plan_id,
        project_arg,
        cwd_arg,
        target_project_arg,
        payload,
    )
    .await
    {
        Ok((path, count)) => AppendOutcome::Written {
            path,
            entry_count: count,
        },
        Err(e) => AppendOutcome::Failed {
            error: e.to_string(),
        },
    }
}

/// Lower-level writer that takes an already-resolved project root and
/// performs the same atomic-rename sidecar append as
/// `append_plan_evidence_entry`. Exists so unit tests can prove the
/// file-shape contract (multi-entry order, `schema_version` persistence,
/// recorded_at stamping) without spinning up a full `AppState`.
///
/// Keep this in lockstep with `super::plan::append_plan_evidence_entry`'s
/// on-disk shape — both write to the same canonical path (`<project_root>/
/// .missiond/v3/runtime/plans/<plan_id>.evidence.json`) so the two writers
/// are interchangeable from a reader's perspective. The AppState-backed
/// writer additionally updates an existing `.missiond/v2/plans` legacy
/// sidecar in place when one is already present.
///
/// `#[allow(dead_code)]`: only invoked by `#[cfg(test)]` tests in this
/// module (`sidecar_append_preserves_order_and_schema_version`,
/// `sidecar_append_surfaces_writer_failure`,
/// `sidecar_append_is_strictly_additive`). Production callers go through
/// `append(...)` which delegates to `super::plan::append_plan_evidence_entry`
/// (resolves project root via the canonical resolver). This twin exists so
/// the on-disk shape contract is testable without standing up a full
/// `AppState` + project registry.
#[allow(dead_code)]
pub(crate) fn append_entry_to_project_root(
    project_root: &Path,
    plan_id: uuid::Uuid,
    entry: Value,
) -> Result<(PathBuf, usize)> {
    let dir = project_root.join(COMPANION_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| anyhow!("mkdir {}: {}", dir.display(), e))?;
    let path = dir.join(format!("{}.evidence.json", plan_id));

    let mut bundle = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow!("read {}: {}", path.display(), e))?;
        serde_json::from_str::<Value>(&raw)
            .unwrap_or_else(|_| json!({"plan_id": plan_id, "entries": []}))
    } else {
        json!({"plan_id": plan_id, "entries": []})
    };

    let stamped = match entry {
        Value::Object(mut map) => {
            map.insert("recorded_at".to_string(), json!(iso_now()));
            Value::Object(map)
        }
        other => json!({ "recorded_at": iso_now(), "evidence": other }),
    };

    if let Some(arr) = bundle.get_mut("entries").and_then(|v| v.as_array_mut()) {
        arr.push(stamped);
    } else {
        bundle["entries"] = json!([stamped]);
    }

    let entry_count = bundle
        .get("entries")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let body = serde_json::to_string_pretty(&bundle)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes()).map_err(|e| anyhow!("write tmp: {}", e))?;
    std::fs::rename(&tmp, &path).map_err(|e| anyhow!("rename: {}", e))?;
    Ok((path, entry_count))
}

/// `#[allow(dead_code)]`: only called by [`append_entry_to_project_root`]
/// (test-only writer; see its docstring). Production callers reach the same
/// stamping behaviour through `super::plan::append_plan_evidence_entry`
/// which keeps a private copy.
#[allow(dead_code)]
fn iso_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}
