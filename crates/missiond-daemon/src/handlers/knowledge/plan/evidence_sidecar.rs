use super::super::evidence_collector;
use super::*;

// ───────────────────────────────────────────────────────────────────────
// record_evidence — persist sidecar JSON next to companion logs
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_record_evidence(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;
    let evidence = args.get("evidence").cloned().ok_or_else(|| {
        anyhow!("`evidence` required (object/array; tool_calls/event_log/test/exec refs)")
    })?;

    let ensured = state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    if ensured.is_none() {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::NOT_FOUND,
            format!("plan `{}` not found", id),
        )));
    }

    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    // wave-12 :: evidence-collector v0 — `evidence_kind` + `source` are
    // additive opt-in stamps. When BOTH are absent the historical wire form
    // is preserved byte-for-byte (`{"evidence": …}`), so legacy callers
    // keep working. When EITHER is present we route through the typed
    // collector wrapper so the new entry carries `schema_version` /
    // canonical `source` / canonical `kind` alongside the original
    // `evidence` body.
    let evidence_kind = args.get("evidence_kind").and_then(|v| v.as_str());
    let source_override = args.get("source").and_then(|v| v.as_str());
    let entry = if evidence_kind.is_some() || source_override.is_some() {
        evidence_collector::wrap_legacy_record_evidence(evidence, evidence_kind, source_override)
    } else {
        json!({ "evidence": evidence })
    };

    let (path, entry_count) = match append_plan_evidence_entry(
        state,
        id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            // Resolver / write failure is a structured rejection rather than an
            // anyhow bubble, so the caller sees the actionable suggestion
            // (intent-worker.lisp :: project-root-spawn-cwd contract).
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, e.to_string()).with_suggestion(
                    "supply project=<registered id> | target_project=<registered id> | cwd=<absolute path>",
                ),
            ));
        }
    };

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "plan_id": id,
        "path": path.display().to_string(),
        "entry_count": entry_count,
        // Echo what the caller asked for so the response makes the routing
        // visible. `null` when the legacy untagged shape was used.
        "evidence_kind": evidence_kind,
        "source": source_override,
    })))
}

/// Append a single evidence entry to
/// `<project_root>/.missiond/v3/runtime/plans/<plan_id>.evidence.json`.
///
/// Existing legacy sidecars under `.missiond/v2/plans` are updated in place so
/// old plan runs remain append-only and readable during the V3 convergence.
///
/// `entry` is merged with a `recorded_at` timestamp. Returns the sidecar path
/// and the resulting total entry count for caller-facing reporting. Used by
/// both `record_evidence` (manual evidence) and the plan-runner internal
/// dispatch path (`plan_runner_dispatch` audit trail).
///
/// Project root resolution goes through [`resolve_project_root`] which
/// honours the canonical contract: explicit `project_id` / absolute `cwd` /
/// fallback `target_project` only. There is **no** process-cwd fallback —
/// callers that omit all signals get a structured error so the evidence
/// sidecar never lands under a surprising directory.
pub(crate) async fn append_plan_evidence_entry(
    state: &AppState,
    plan_id: uuid::Uuid,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    entry: Value,
) -> Result<(PathBuf, usize)> {
    let project_root = resolve_project_root(
        &state.project_registry,
        project_arg,
        cwd_arg,
        target_project_arg,
    )
    .await?;
    let path = existing_plan_evidence_sidecar_path(&project_root, plan_id);
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("cannot resolve parent for {}", path.display()))?;
    std::fs::create_dir_all(&dir).map_err(|e| anyhow!("mkdir {}: {}", dir.display(), e))?;

    let mut bundle = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow!("read {}: {}", path.display(), e))?;
        serde_json::from_str::<Value>(&raw)
            .unwrap_or_else(|_| json!({"plan_id": plan_id, "entries": []}))
    } else {
        json!({"plan_id": plan_id, "entries": []})
    };

    // Stamp recorded_at on the entry. If caller already supplied an object,
    // merge the field; otherwise wrap the value under `evidence`.
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
