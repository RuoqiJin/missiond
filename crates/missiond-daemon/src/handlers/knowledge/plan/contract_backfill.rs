use super::*;

const DEFAULT_BACKFILL_LIMIT: i64 = 100;
const MAX_BACKFILL_LIMIT: i64 = 1000;

pub(super) async fn action_backfill_contracts(
    state: &AppState,
    args: &Value,
) -> Result<ToolResult> {
    let apply = args.get("apply").and_then(Value::as_bool).unwrap_or(false);
    let include_terminal = args
        .get("include_terminal")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let limit = args
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_BACKFILL_LIMIT)
        .clamp(1, MAX_BACKFILL_LIMIT);
    let status = args
        .get("status")
        .and_then(Value::as_str)
        .map(|s| PlanStatus::from_str(s).map_err(|e| anyhow!(e)))
        .transpose()?;

    let rows = state
        .store
        .plan_list_contract_backfill_candidates(status, include_terminal, limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let scanned = rows.len();
    let mut updated = 0usize;
    let mut skipped = 0usize;
    let mut candidates = Vec::new();
    let mut errors = Vec::new();

    for plan in rows {
        if !plan_contract_json_requires_projection(&plan.contract_json) {
            skipped += 1;
            continue;
        }
        candidates.push(json!({
            "plan_id": plan.id,
            "version": plan.version,
            "status": plan.status.as_str(),
            "board_task_id": plan.board_task_id,
        }));
        let projected = match plan_contract_json_from_sexp(&plan.sexp_text) {
            Ok(projected) => projected,
            Err(err) => {
                errors.push(json!({
                    "plan_id": plan.id,
                    "reason": format!("missiond-lispc emit-plan-contract failed: {}", err),
                }));
                continue;
            }
        };
        if !apply {
            continue;
        }
        if let Err(err) = state
            .store
            .plan_update_contract_json(plan.id, &projected)
            .await
        {
            errors.push(json!({
                "plan_id": plan.id,
                "reason": format!("plan_update_contract_json failed: {}", err),
            }));
            continue;
        }
        updated += 1;
    }

    Ok(ToolResult::json_pretty(&json!({
        "action": "backfill_contracts",
        "apply": apply,
        "scanned": scanned,
        "candidates": candidates.len(),
        "updated": updated,
        "skipped": skipped,
        "errors": errors,
        "candidate_plans": candidates,
        "remaining_estimate": if scanned as i64 == limit { Value::String("unknown_more_possible".to_string()) } else { Value::Null },
    })))
}
