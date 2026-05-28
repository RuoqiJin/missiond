use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::json;

pub(crate) const FULL_OS_ENV: &str = "MISSIOND_FULL_OS_ENABLE";
pub(crate) const WORKFLOW_ENV: &str = "MISSIOND_FEATURE_WORKFLOW_ENABLE";
pub(crate) const MEMORY_ENV: &str = "MISSIOND_FEATURE_MEMORY_ENABLE";
pub(crate) const SKILL_STORE_ENV: &str = "MISSIOND_FEATURE_SKILL_STORE_ENABLE";
pub(crate) const ROUTER_EXPERIMENTS_ENV: &str = "MISSIOND_FEATURE_ROUTER_EXPERIMENTS_ENABLE";
pub(crate) const CODEX_REPLAY_ENV: &str = "MISSIOND_FEATURE_CODEX_REPLAY_ENABLE";
pub(crate) const SELF_EVOLUTION_ENV: &str = "MISSIOND_FEATURE_SELF_EVOLUTION_ENABLE";
pub(crate) const CONVERSATIONS_ENV: &str = "MISSIOND_FEATURE_CONVERSATIONS_ENABLE";
pub(crate) const INFRA_OS_ENV: &str = "MISSIOND_FEATURE_INFRA_OS_ENABLE";
pub(crate) const BOARD_ADVANCED_ENV: &str = "MISSIOND_FEATURE_BOARD_ADVANCED_ENABLE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionalFeature {
    pub(crate) id: &'static str,
    pub(crate) env: &'static str,
    pub(crate) layer: &'static str,
    pub(crate) reason: &'static str,
}

impl OptionalFeature {
    pub(crate) const fn new(
        id: &'static str,
        env: &'static str,
        layer: &'static str,
        reason: &'static str,
    ) -> Self {
        Self {
            id,
            env,
            layer,
            reason,
        }
    }

    pub(crate) fn enabled(self) -> bool {
        optional_feature_enabled(self.env)
    }
}

pub(crate) fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn full_os_enabled() -> bool {
    env_truthy(FULL_OS_ENV)
}

pub(crate) fn optional_feature_enabled(env_key: &str) -> bool {
    full_os_enabled() || env_truthy(env_key)
}

pub(crate) fn disabled_tool_result(tool_name: &str, feature: OptionalFeature) -> ToolResult {
    ToolResult::structured_error(
        ToolError::new(
            error_codes::FEATURE_DISABLED,
            format!(
                "{tool_name} belongs to optional MissionD layer `{}` and is disabled in kernel-core mode",
                feature.id
            ),
        )
        .with_details(json!({
            "schema": "missiond.feature-disabled.v1",
            "tool": tool_name,
            "feature": feature.id,
            "layer": feature.layer,
            "enable_env": feature.env,
            "enable_all_env": FULL_OS_ENV,
            "reason": feature.reason,
        }))
        .with_suggestion(format!(
            "enable {}=true or {}=true when running MissionD full-os; otherwise use the kernel core path",
            feature.env, FULL_OS_ENV
        )),
    )
}

pub(crate) fn optional_feature_for_tool(name: &str) -> Option<OptionalFeature> {
    if workflow_tool(name) {
        return Some(OptionalFeature::new(
            "workflow",
            WORKFLOW_ENV,
            "full-os",
            "plan DAG, workflow, review-gate, cascade, swarm, and agent-execution are optional orchestration layers",
        ));
    }
    if memory_tool(name) {
        return Some(OptionalFeature::new(
            "memory",
            MEMORY_ENV,
            "full-os",
            "KB, memory extraction, embeddings, insight, and context-gather are optional knowledge layers",
        ));
    }
    if skill_store_tool(name) {
        return Some(OptionalFeature::new(
            "skill-store",
            SKILL_STORE_ENV,
            "full-os",
            "skill-store and skill execution are optional plugin layers",
        ));
    }
    if router_experiment_tool(name) {
        return Some(OptionalFeature::new(
            "router-experiments",
            ROUTER_EXPERIMENTS_ENV,
            "full-os",
            "multi-provider router experiments are outside the kernel core",
        ));
    }
    if name == "mission_codex_replay" {
        return Some(OptionalFeature::new(
            "codex-replay",
            CODEX_REPLAY_ENV,
            "full-os",
            "Codex replay is an optional diagnostic/research layer",
        ));
    }
    if name == "mission_nightly_evolution" {
        return Some(OptionalFeature::new(
            "self-evolution",
            SELF_EVOLUTION_ENV,
            "full-os",
            "nightly evolution and Lisp code sync are proposal-only optional layers",
        ));
    }
    if conversation_tool(name) {
        return Some(OptionalFeature::new(
            "advanced-conversations",
            CONVERSATIONS_ENV,
            "full-os",
            "conversation history, retrospectives, labels, and advanced Board conversation views are optional projections",
        ));
    }
    if infra_os_tool(name) {
        return Some(OptionalFeature::new(
            "infra-os",
            INFRA_OS_ENV,
            "full-os",
            "infra, daemon update, power, and external OS operations are optional operations layers",
        ));
    }
    if board_advanced_tool(name) {
        return Some(OptionalFeature::new(
            "advanced-board",
            BOARD_ADVANCED_ENV,
            "full-os",
            "Board decomposition is an optional orchestration helper; core Board projection remains enabled",
        ));
    }
    None
}

fn workflow_tool(name: &str) -> bool {
    matches!(
        name,
        "mission_plan"
            | "mission_workflow"
            | "mission_request"
            | "mission_directive"
            | "mission_execution"
            | "mission_flow_run"
            | "mission_submit_phase_result"
            | "mission_swarm_run"
            | "mission_forge_build"
            | "mission_forge_lint"
            | "mission_universe_graph"
            | "mission_cascade_plan"
            | "mission_cascade_trigger"
            | "mission_cascade_lint"
    )
}

fn memory_tool(name: &str) -> bool {
    name == "mission_memory"
        || name == "mission_insight"
        || name == "mission_embedding_ops"
        || name == "mission_context_boot"
        || name == "mission_context_gather"
        || name == "mission_code_search"
        || name == "mission_embedding_stats"
        || name == "mission_habit_scan"
        || name.starts_with("mission_kb_")
        || name.starts_with("mission_beacon_")
}

fn skill_store_tool(name: &str) -> bool {
    name.starts_with("mission_skill_")
        || matches!(
            name,
            "mission_context_build" | "mission_context_resolve" | "mission_skill_context"
        )
}

fn router_experiment_tool(name: &str) -> bool {
    name.starts_with("mission_router_chat")
        || name.starts_with("mission_minimax_")
        || name.starts_with("mission_sonnet_")
        || name.starts_with("mission_cc_")
        || name.starts_with("mission_gemini_")
        || matches!(name, "mission_llm_trace" | "mission_gemini_auth")
}

fn conversation_tool(name: &str) -> bool {
    name.starts_with("mission_conversation_")
        || name == "mission_agent_trajectory"
        || name == "mission_trigger_backfill"
        || name == "mission_retrospective_manage"
        || name == "mission_retrospective"
        || name == "mission_retrospective_list"
        || name == "mission_retrospective_backfill"
        || name == "mission_message_search"
        || name == "mission_context_around"
        || name == "mission_user_message_index"
        || name == "mission_activity_report"
}

fn infra_os_tool(name: &str) -> bool {
    name.starts_with("xjp_")
        || name.starts_with("mission_infra_")
        || matches!(
            name,
            "mission_reachability"
                | "mission_os_diagnose"
                | "mission_daemon_update"
                | "mission_power_control"
                | "mission_global_instruction"
                | "mission_code_map_graph"
        )
}

fn board_advanced_tool(name: &str) -> bool {
    name == "mission_board_decompose"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_tools_are_not_optional() {
        for tool in [
            "mission_task_delegate",
            "mission_compute_slot",
            "mission_shared_memory",
            "mission_board_create",
            "mission_board_update",
            "mission_job_poll",
            "mission_pty_spawn",
        ] {
            assert_eq!(optional_feature_for_tool(tool), None, "{tool}");
        }
    }

    #[test]
    fn non_core_tools_are_feature_gated() {
        let cases = [
            ("mission_plan", WORKFLOW_ENV),
            ("mission_workflow", WORKFLOW_ENV),
            ("mission_submit_phase_result", WORKFLOW_ENV),
            ("mission_memory", MEMORY_ENV),
            ("mission_kb_query", MEMORY_ENV),
            ("mission_skill_exec", SKILL_STORE_ENV),
            ("mission_router_chat", ROUTER_EXPERIMENTS_ENV),
            ("mission_codex_replay", CODEX_REPLAY_ENV),
            ("mission_nightly_evolution", SELF_EVOLUTION_ENV),
            ("mission_conversation_query", CONVERSATIONS_ENV),
            ("mission_infra_ops", INFRA_OS_ENV),
            ("mission_board_decompose", BOARD_ADVANCED_ENV),
        ];
        for (tool, env) in cases {
            assert_eq!(
                optional_feature_for_tool(tool).map(|f| f.env),
                Some(env),
                "{tool}"
            );
        }
    }
}
