use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_compute_slot",
        "Manage dynamic compute slots — ephemeral workstations created on demand with automatic TTL lifecycle.\n\n\
         Actions:\n\
         - create: Spawn a new dynamic slot from a predefined template. Requires `template` (e.g. \"coder\", \"ops\"). \
           Optional: `objective` (intent description), `cwd` (working directory), `max_ttl` (seconds, default 14400 = 4h, max 28800 = 8h).\n\
         - terminate: Gracefully stop a dynamic slot. Requires `slot_id`.\n\
         - extend: Extend a slot's TTL (max 2 extensions). Requires `slot_id` and `additional_seconds` (max 3600 per extension).\n\
         - list: List all dynamic slots (merged with static slots). Optional `status` filter.\n\n\
         Constraints: max 5 active dynamic slots, dangerouslySkipPermissions always false, \
         only Jarvis (root agent) can create slots.",
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "terminate", "extend", "list"],
                    "description": "Operation to perform"
                },
                "template": {
                    "type": "string",
                    "description": "Template ID for create (e.g. 'coder', 'ops')"
                },
                "objective": {
                    "type": "string",
                    "description": "Intent description for the slot (create only)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory override (create only). Must be under allowed prefixes."
                },
                "max_ttl": {
                    "type": "integer",
                    "description": "Max TTL in seconds (create only). Default 14400 (4h), max 28800 (8h)."
                },
                "slot_id": {
                    "type": "string",
                    "description": "Dynamic slot ID (terminate/extend)"
                },
                "additional_seconds": {
                    "type": "integer",
                    "description": "Seconds to extend TTL (extend only, max 3600 per extension)"
                },
                "status": {
                    "type": "string",
                    "description": "Filter by status (list only): active, terminated"
                }
            },
            "required": ["action"]
        }),
    )]
}
