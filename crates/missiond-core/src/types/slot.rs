use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ============ CLI Engine ============

/// CLI engine type for slot workstations.
/// Determines which binary to spawn and which state parser to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CliEngine {
    /// Anthropic Claude Code (interactive PTY mode)
    #[default]
    ClaudeCode,
    /// Google Gemini CLI (interactive PTY or subprocess stream-json)
    Gemini,
    /// OpenAI Codex CLI (subprocess JSON mode)
    Codex,
}

impl std::fmt::Display for CliEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliEngine::ClaudeCode => write!(f, "claude_code"),
            CliEngine::Gemini => write!(f, "gemini"),
            CliEngine::Codex => write!(f, "codex"),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_start: Option<bool>,
    /// Skip all permission prompts and trust dialogs (--dangerously-skip-permissions)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dangerously_skip_permissions: Option<bool>,
    /// Declarative traits controlling pipeline behavior.
    /// If empty/absent, defaults are inferred from role at load time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<SlotTrait>,
    /// Custom environment variables injected into the PTY child process.
    /// Supports `${secret:path}` syntax for Secret Store resolution.
    /// Used for per-slot model provider configuration (e.g., MiniMax M2.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

impl SlotConfig {
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

/// Slot = Config + session (process state managed by ProcessManager)
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
