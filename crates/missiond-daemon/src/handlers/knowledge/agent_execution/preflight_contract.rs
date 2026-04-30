use serde_json::{json, Value};
use std::path::Path;

use super::preflight_scope::evaluate_task_contract_for_preflight;

pub(super) fn apply_task_contract_projection(summary: &mut Value, root: &Path, args: &Value) {
    let Some(tcp) = args
        .get("task_contract_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    else {
        return;
    };

    summary["task_contract_path"] = json!(tcp);
    let staged: Vec<String> = summary
        .get("staged_files")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let changed: Vec<String> = summary
        .get("changed_files")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let (status, scope_summary, resolved_path, failure) =
        evaluate_task_contract_for_preflight(root, tcp, &staged, &changed);
    summary["task_contract_status"] = json!(status);
    if let Some(rp) = resolved_path {
        summary["task_contract_resolved_path"] = json!(rp);
    }
    if let Some(scope) = scope_summary {
        // Promote the four contract-derived fields to the top level so
        // dashboards keying off `task_contract_status` can read the drift
        // signals without descending one more level. The full projection
        // stays under `task_contract_scope` for inspectors that want the raw
        // inputs.
        for key in [
            "staged_out_of_scope",
            "staged_forbidden",
            "unstaged_in_scope",
        ] {
            if let Some(v) = scope.get(key) {
                summary[key] = v.clone();
            }
        }

        // Override `next_step` with the contract-aware hint when the contract
        // added forbidden / out-of-scope drift the claim-only check missed.
        let has_contract_drift = scope
            .get("staged_forbidden")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false)
            || scope
                .get("staged_out_of_scope")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
        if has_contract_drift {
            if let Some(ns) = scope.get("next_step") {
                summary["next_step"] = ns.clone();
            }
            summary["ok"] = json!(false);
        }
        summary["task_contract_scope"] = scope;
    } else if let Some(msg) = failure {
        summary["task_contract_error"] = json!(msg);
    }
}
