use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== mission_forge_build =====
        ToolDefinition::new(
            "mission_forge_build",
            "Run Forge Lisp→Rust stamping for a registered project. Reads intent.lisp and regenerates all generated.rs files. Use dry_run=true to preview without writing.",
            json!({
                "type": "object",
                "required": ["project"],
                "properties": {
                    "project": {"type": "string", "description": "Project ID (must be registered)"},
                    "dry_run": {"type": "boolean", "default": false},
                    "output_dir": {"type": "string", "description": "Optional output override"}
                }
            }),
        ),
        // ===== mission_forge_lint =====
        ToolDefinition::new(
            "mission_forge_lint",
            "Run Forge governance lint on a registered project's intent.lisp files. Checks L1/L2/L3 granularity violations and structural rules. Returns raw forge output.",
            json!({
                "type": "object",
                "required": ["project"],
                "properties": {
                    "project": {"type": "string", "description": "Project ID (must be registered)"}
                }
            }),
        ),
    ]
}
