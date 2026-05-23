use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_codex_replay",
        "Codex app-server protocol replay runner. start_campaign/run_once/pause_campaign/resume_campaign/stop_campaign/status/list_runs/get_run",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "start_campaign",
                        "run_once",
                        "pause_campaign",
                        "resume_campaign",
                        "stop_campaign",
                        "status",
                        "list_runs",
                        "get_run"
                    ]
                },
                "campaignId": {
                    "type": "string",
                    "description": "Campaign id for pause/resume/stop/status/list_runs/get_run. Defaults to latest campaign where safe."
                },
                "runId": {
                    "type": "string",
                    "description": "[get_run] Replay run id."
                },
                "projectRoot": {
                    "type": "string",
                    "description": "[start_campaign/run_once] Project root. Defaults to daemon current directory."
                },
                "maxCycles": {
                    "type": "integer",
                    "description": "[start_campaign] Optional max cycles. Omit for continuous loop."
                },
                "intervalSeconds": {
                    "type": "integer",
                    "description": "[start_campaign] Seconds to wait between completed cycles. Default 0."
                },
                "limit": {
                    "type": "integer",
                    "description": "[status/list_runs/get_run] Max rows. Default 20."
                }
            }
        }),
    )]
}
