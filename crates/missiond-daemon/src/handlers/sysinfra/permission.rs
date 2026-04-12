use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

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

#[derive(Deserialize)]
struct MergedForSlotArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tools
    if name == "mission_permission_query" {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("get");
        return match action {
            "get" => handle_inner(state, "mission_permission_get", args).await,
            "learned_list" => handle_inner(state, "mission_permission_learned_list", args).await,
            "merged_for_slot" => {
                handle_inner(state, "mission_permission_merged_for_slot", args).await
            }
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    if name == "mission_permission_mutate" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("reload");
        return match action {
            "set_role" => handle_inner(state, "mission_permission_set_role", args).await,
            "set_slot" => handle_inner(state, "mission_permission_set_slot", args).await,
            "auto_allow" => handle_inner(state, "mission_permission_add_auto_allow", args).await,
            "reload" => handle_inner(state, "mission_permission_reload", args).await,
            "revoke" => handle_inner(state, "mission_permission_learned_revoke", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Permission =====
        "mission_permission_get" => Ok(ToolResult::json_pretty(&state.permission.get_config())),
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

            let learned = state
                .permission
                .learned()
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

            let learned = state
                .permission
                .learned()
                .ok_or_else(|| anyhow!("Learned permissions not initialized"))?;

            let deleted = learned.forget(&scope_type, &scope_id, &tool_pattern)?;
            Ok(ToolResult::json(&serde_json::json!({
                "success": true,
                "deleted": deleted,
            })))
        }

        // ===== Merged-for-slot view =====
        // Returns the union of all permission entries actually visible to a slot
        // at spawn time: global + role + project (resolved from cwd) + slot scope,
        // with slot-scoped rules winning on (tool_pattern, param_pattern) dedup.
        // Also includes the slot's static role/slot rules from PermissionPolicy
        // so callers can see the full picture at a glance.
        "mission_permission_merged_for_slot" => {
            let MergedForSlotArgs { slot_id } = serde_json::from_value(args)?;

            let learned = state
                .permission
                .learned()
                .ok_or_else(|| anyhow!("Learned permissions not initialized"))?;

            // Look up the slot's role and cwd from the mission control registry.
            let slot = state
                .mission
                .list_slots()
                .into_iter()
                .find(|s| s.config.id == slot_id)
                .ok_or_else(|| anyhow!("Slot not found: {}", slot_id))?;
            let role = slot.config.role.clone();
            let cwd_str = slot.config.cwd.clone();

            let project_id = if let Some(cwd) = cwd_str.as_deref() {
                state
                    .project_registry
                    .read()
                    .await
                    .resolve(cwd)
                    .map(|s| s.to_string())
            } else {
                None
            };

            let merged =
                learned.get_for_spawn(&role, project_id.as_deref(), Some(slot_id.as_str()))?;
            let static_role_rule = state.permission.get_role_rule(&role);
            let static_slot_rule = state.permission.get_slot_rule(&slot_id);

            Ok(ToolResult::json_pretty(&serde_json::json!({
                "slotId": slot_id,
                "role": role,
                "cwd": cwd_str,
                "projectId": project_id,
                "learned": merged,
                "staticRoleRule": static_role_rule,
                "staticSlotRule": static_slot_rule,
            })))
        }

        _ => Err(anyhow!("Unknown permission tool: {name}")),
    }
}
