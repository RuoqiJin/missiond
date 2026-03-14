use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use missiond_core::PermissionRule;

#[derive(Deserialize)]
struct SetRolePermissionArgs {
    role: String,
    rule: PermissionRule,
}

#[derive(Deserialize)]
struct SetSlotPermissionArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    rule: PermissionRule,
}

#[derive(Deserialize)]
struct AddAutoAllowArgs {
    role: Option<String>,
    #[serde(rename = "slotId")]
    slot_id: Option<String>,
    pattern: String,
}

#[derive(Deserialize)]
struct GetLearnedArgs {
    #[serde(rename = "scopeType")]
    scope_type: Option<String>,
    #[serde(rename = "scopeId")]
    scope_id: Option<String>,
}

#[derive(Deserialize)]
struct LearnedRevokeArgs {
    #[serde(rename = "scopeType")]
    scope_type: String,
    #[serde(rename = "scopeId")]
    scope_id: String,
    #[serde(rename = "toolPattern")]
    tool_pattern: String,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Permission =====
        "mission_permission_get" => Ok(ToolResult::json_pretty(
            &state.permission.get_config(),
        )),
        "mission_permission_set_role" => {
            let SetRolePermissionArgs { role, rule } = serde_json::from_value(args)?;
            state.permission.set_role_rule(&role, rule.clone());
            Ok(ToolResult::json(&serde_json::json!({
                "success": true,
                "role": role,
                "rule": rule,
            })))
        }
        "mission_permission_set_slot" => {
            let SetSlotPermissionArgs { slot_id, rule } = serde_json::from_value(args)?;
            state.permission.set_slot_rule(&slot_id, rule.clone());
            Ok(ToolResult::json(&serde_json::json!({
                "success": true,
                "slotId": slot_id,
                "rule": rule,
            })))
        }
        "mission_permission_add_auto_allow" => {
            let AddAutoAllowArgs {
                role,
                slot_id,
                pattern,
            } = serde_json::from_value(args)?;
            if let Some(role) = role {
                state.permission.add_role_auto_allow(&role, &pattern);
                Ok(ToolResult::json(&serde_json::json!({
                    "success": true,
                    "role": role,
                    "pattern": pattern,
                })))
            } else if let Some(slot_id) = slot_id {
                state.permission.add_slot_auto_allow(&slot_id, &pattern);
                Ok(ToolResult::json(&serde_json::json!({
                    "success": true,
                    "slotId": slot_id,
                    "pattern": pattern,
                })))
            } else {
                Ok(ToolResult::error("Must specify role or slotId"))
            }
        }
        "mission_permission_reload" => {
            state.permission.reload();
            Ok(ToolResult::json(&serde_json::json!({ "success": true })))
        }

        // ===== Learned Permissions =====
        "mission_permission_learned_list" => {
            let GetLearnedArgs {
                scope_type,
                scope_id,
            } = serde_json::from_value(args)?;

            let learned = state.permission.learned()
                .ok_or_else(|| anyhow!("Learned permissions not initialized"))?;

            let result = if let (Some(st), Some(si)) = (scope_type, scope_id) {
                learned.get_for_scope(&st, &si)?
            } else {
                learned.get_all()?
            };
            Ok(ToolResult::json_pretty(&result))
        }
        "mission_permission_learned_revoke" => {
            let LearnedRevokeArgs {
                scope_type,
                scope_id,
                tool_pattern,
            } = serde_json::from_value(args)?;

            let learned = state.permission.learned()
                .ok_or_else(|| anyhow!("Learned permissions not initialized"))?;

            let deleted = learned.forget(&scope_type, &scope_id, &tool_pattern)?;
            Ok(ToolResult::json(&serde_json::json!({
                "success": true,
                "deleted": deleted,
            })))
        }

        _ => Err(anyhow!("Unknown permission tool: {name}")),
    }
}
