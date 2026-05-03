use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use missiond_shared::CliEngine;

// ============ Lifecycle ============

/// Agent lifecycle mode — determines how the daemon manages the slot's PTY session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    /// Daemon auto-starts on boot and auto-restarts on crash.
    #[default]
    Persistent,
    /// Started on demand (task dispatch, manual spawn). No auto-restart.
    OnDemand,
}

impl std::fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lifecycle::Persistent => write!(f, "persistent"),
            Lifecycle::OnDemand => write!(f, "on_demand"),
        }
    }
}

// ============ Slot Config ============

/// Slot traits: declarative capabilities that control pipeline routing.
/// Used to determine which slots' conversations enter extraction pipelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotTrait {
    /// Slot is a system/meta agent (memory extraction, code review, GC, etc.).
    /// Its conversations are excluded from all extraction pipelines.
    IsMetaAgent,
    /// Slot produces conversations that should be analyzed for knowledge extraction.
    GeneratesKnowledge,
    /// Slot supports native image/vision input (e.g., Codex CLI `-i`).
    SupportsVision,
    /// Slot supports MCP tool server connections (e.g., Claude Code `--mcp-config`).
    SupportsMcp,
}

/// Configuration for a slot (workstation)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotConfig {
    pub id: String,
    pub role: String,
    pub description: String,
    /// CLI engine type. Defaults to ClaudeCode for backward compatibility.
    #[serde(default)]
    pub engine: CliEngine,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Resolved canonical project root for project-bound CLI slot spawn.
    /// Set by `mission_pty_spawn` / `mission_compute_slot` / `mission_task_delegate` /
    /// `mission_agent` after running the project-root resolver. When set,
    /// `spawn_tracked_slot` uses it as the process cwd. See
    /// `intent-worker.lisp :: invariant project-root-spawn-cwd`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "project_root"
    )]
    pub project_root: Option<String>,
    /// Caller-supplied cwd preserved for prompt/context/audit only. Never used
    /// as process cwd after root resolution. See worker pillar slot-config-fields.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "requested_cwd"
    )]
    pub requested_cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "mcp_config")]
    pub mcp_config: Option<String>,
    /// Agent lifecycle mode. If set, takes precedence over `auto_start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<Lifecycle>,
    /// Legacy auto-start flag. Prefer `lifecycle: persistent` in new configs.
    #[serde(skip_serializing_if = "Option::is_none", alias = "auto_start")]
    pub auto_start: Option<bool>,
    /// Skip all permission prompts and trust dialogs (--dangerously-skip-permissions)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "dangerously_skip_permissions"
    )]
    pub dangerously_skip_permissions: Option<bool>,
    /// Model override for the CLI session (e.g., "sonnet", "opus", "haiku").
    /// Passed as the provider-specific model flag when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Declarative profile used to resolve model/runtime options from Lisp.
    /// The resolved `model` remains the CLI argument; this is preserved for
    /// observability and downstream projection.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "model_profile"
    )]
    pub model_profile: Option<String>,
    /// Provider-specific reasoning effort. Codex projects this as
    /// `-c model_reasoning_effort="<value>"`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "reasoning_effort"
    )]
    pub reasoning_effort: Option<String>,
    /// Enable provider web/search mode when supported.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "search_enabled"
    )]
    pub search_enabled: Option<bool>,
    /// Provider sandbox profile, e.g. Codex `read-only` or `workspace-write`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    /// Provider approval profile, e.g. Codex `never` or `on-request`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "approval_policy"
    )]
    pub approval_policy: Option<String>,
    /// Provider tool policy file, e.g. Gemini CLI `--policy`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "tool_policy_path"
    )]
    pub tool_policy_path: Option<String>,
    /// Declarative traits controlling pipeline behavior.
    /// If empty/absent, defaults are inferred from role at load time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<SlotTrait>,
    /// The conversation category this slot's logs should be routed to (e.g. 'worker', 'meta', 'jarvis').
    /// If not provided, it falls back to 'worker' (or 'meta' if IsMetaAgent trait is present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Custom environment variables injected into the PTY child process.
    /// Supports `${secret:path}` syntax for Secret Store resolution.
    /// Used for per-slot model provider configuration (e.g., MiniMax M2.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    /// Prompt injected as the first message after the slot reaches Idle.
    /// Used for persistent slots that need a standing instruction on boot.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "initial_prompt"
    )]
    pub initial_prompt: Option<String>,
}

impl SlotConfig {
    /// Should this slot be auto-started on daemon boot and auto-restarted on crash?
    /// Checks `lifecycle` first (new), falls back to `auto_start` (legacy).
    pub fn is_persistent(&self) -> bool {
        match self.lifecycle {
            Some(Lifecycle::Persistent) => true,
            Some(Lifecycle::OnDemand) => false,
            None => self.auto_start == Some(true),
        }
    }

    /// Is this a system/meta agent whose conversations should be excluded from pipelines?
    pub fn is_meta_agent(&self) -> bool {
        self.traits.contains(&SlotTrait::IsMetaAgent)
    }

    /// Does this slot support native vision/image input?
    pub fn supports_vision(&self) -> bool {
        self.traits.contains(&SlotTrait::SupportsVision)
    }

    /// Does this slot support MCP tool server connections?
    pub fn supports_mcp(&self) -> bool {
        self.traits.contains(&SlotTrait::SupportsMcp)
    }

    /// Apply default traits based on role and engine.
    /// Role-based defaults only apply when no traits are explicitly configured.
    /// Engine-based capabilities are always injected (idempotent).
    pub fn apply_default_traits(&mut self) {
        // Role-based defaults (only if no explicit traits)
        if self.traits.is_empty() {
            match self.role.as_str() {
                "memory" => self.traits.push(SlotTrait::IsMetaAgent),
                _ => {}
            }
        }

        // Apply fallback category if none provided
        if self.category.is_none() {
            // Check hardcoded IDs for historical compatibility if needed, but primarily use traits/role
            if self.is_meta_agent() || self.id == "slot-memory" || self.id == "slot-memory-slow" {
                self.category = Some("meta".to_string());
            } else if self.id == "slot-jarvis" || self.role == "jarvis" {
                self.category = Some("jarvis".to_string());
            } else {
                self.category = Some("worker".to_string());
            }
        }

        // Engine-based capability injection (always applied, idempotent)
        match self.engine {
            CliEngine::ClaudeCode => {
                if !self.traits.contains(&SlotTrait::SupportsMcp) {
                    self.traits.push(SlotTrait::SupportsMcp);
                }
            }
            CliEngine::Codex => {
                if !self.traits.contains(&SlotTrait::SupportsVision) {
                    self.traits.push(SlotTrait::SupportsVision);
                }
            }
            CliEngine::Gemini => {
                // Gemini CLI: no special capabilities yet
            }
        }
    }
}

/// Slot = Config + session (process lifecycle managed by PTYManager)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    #[serde(flatten)]
    pub config: SlotConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

// ============ Config ============

/// Slots configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotsConfig {
    pub slots: Vec<SlotConfig>,
}
