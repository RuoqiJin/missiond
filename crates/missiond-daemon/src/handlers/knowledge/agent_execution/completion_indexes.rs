use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::Value;

use super::claim_lease::{parse_claims, parse_iso};
use super::log_store::{lisp_quote_string, list_block_summaries, update_kv_in_node, LogFile};

pub(super) fn rebuild_derived_indexes(file: &mut LogFile) -> Result<()> {
    let claims = parse_claims(file);
    let now = Utc::now();
    let active_ids: Vec<String> = claims
        .iter()
        .filter(|c| {
            c.status == "active"
                && c.lease_expires_at
                    .as_deref()
                    .and_then(parse_iso)
                    .map(|exp| exp >= now)
                    .unwrap_or(true)
        })
        .map(|c| c.id.clone())
        .collect();

    let open_issue_ids = list_block_summaries(file, "issues", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(Value::String(head.to_string()))
        }
    });

    let unresolved_dev_ids = list_block_summaries(file, "deviations", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(Value::String(head.to_string()))
        }
    });

    let latest_decisions = list_block_summaries(file, "decisions", |_kvs, head| {
        Some(Value::String(head.to_string()))
    });
    let completed_phases = list_block_summaries(file, "completions", |kvs, _head| {
        Some(Value::String(
            kvs.get("phase")
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_default(),
        ))
    });

    let render_list = |items: &[Value]| -> String {
        let parts: Vec<String> = items
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(lisp_quote_string)
            .collect();
        if parts.is_empty() {
            "()".to_string()
        } else {
            format!("({})", parts.join(" "))
        }
    };

    let block = match file.find_block("derived-indexes").cloned() {
        Some(b) => b,
        None => return Ok(()),
    };
    let active_lit = render_list(
        &active_ids
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect::<Vec<_>>(),
    );
    let issues_lit = render_list(&open_issue_ids);
    let dev_lit = render_list(&unresolved_dev_ids);
    let dec_lit = render_list(&latest_decisions);
    let phases_lit = render_list(&completed_phases);

    update_kv_in_node(file, &block, "active-claims", &active_lit)?;
    let block2 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block2, "open-issues", &issues_lit)?;
    let block3 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block3, "unresolved-deviations", &dev_lit)?;
    let block4 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block4, "latest-decisions", &dec_lit)?;
    let block5 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block5, "completed-phases", &phases_lit)?;
    Ok(())
}
