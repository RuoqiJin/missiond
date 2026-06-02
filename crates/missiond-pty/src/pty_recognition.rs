//! Provider-aware PTY recognition derived from upstream CLI TUI state surfaces.
//!
//! This module is intentionally local to `missiond-pty`: MissionD owns the
//! orchestration semantics even when the terminal UI belongs to Codex, Gemini,
//! or Claude Code. The upstream projects remain the evidence source; this code
//! turns visible PTY text into a stable MissionD recognition snapshot.

use missiond_shared::CliEngine;
use once_cell::sync::Lazy;
use regex::Regex;
use semantic_terminal::{ParserContext, ParserMeta, State, StateDetectionResult, StateParser};
use serde::{Deserialize, Serialize};

use crate::screenshot::{StyledScreenLine, StyledScreenSnapshot, StyledScreenSpan};
use crate::session::SessionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PtyCanonicalState {
    Running,
    Idle,
    Blocked,
    Complete,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyRecognitionSnapshot {
    pub provider: CliEngine,
    pub state: PtyCanonicalState,
    pub confidence: f64,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_identity: Option<ProviderScreenIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_usage: Option<ProviderUsageScreen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_mcp: Option<ProviderMcpScreen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_signals: Option<ProviderScreenSignals>,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderScreenIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderScreenSignals {
    #[serde(default)]
    pub placeholder_visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_text: Option<String>,
    #[serde(default)]
    pub model_picker_visible: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visible_models: Vec<String>,
    #[serde(default)]
    pub permission_picker_visible: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visible_permission_modes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_permission_mode: Option<String>,
    #[serde(default)]
    pub startup_prompt_visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_prompt_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visible_startup_options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_startup_option: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_startup_option_index: Option<u16>,
    #[serde(default)]
    pub selected_startup_option_checked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tool_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tool_label: Option<String>,
    #[serde(default)]
    pub web_search_active: bool,
    #[serde(default)]
    pub folded_tool_output: bool,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub separator_count: usize,
}

impl ProviderScreenSignals {
    fn is_empty(&self) -> bool {
        !self.placeholder_visible
            && self.placeholder_text.is_none()
            && !self.model_picker_visible
            && self.visible_models.is_empty()
            && !self.permission_picker_visible
            && self.visible_permission_modes.is_empty()
            && self.selected_permission_mode.is_none()
            && !self.startup_prompt_visible
            && self.startup_prompt_kind.is_none()
            && self.visible_startup_options.is_empty()
            && self.selected_startup_option.is_none()
            && self.selected_startup_option_index.is_none()
            && !self.selected_startup_option_checked
            && self.last_user_message.is_none()
            && self.last_assistant_message.is_none()
            && self.last_tool_kind.is_none()
            && self.last_tool_label.is_none()
            && !self.web_search_active
            && !self.folded_tool_output
            && self.separator_count == 0
    }
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

impl ProviderScreenIdentity {
    fn is_empty(&self) -> bool {
        self.cli_version.is_none()
            && self.account.is_none()
            && self.plan.is_none()
            && self.current_model.is_none()
            && self.reasoning_effort.is_none()
            && self.permission_mode.is_none()
            && self.selected_model.is_none()
            && self.cwd.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageScreen {
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_quotas: Vec<ProviderModelQuota>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_range: Option<ProviderVisibleRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelQuota {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderVisibleRange {
    pub start: u16,
    pub end: u16,
    pub total: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMcpScreen {
    pub title: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<ProviderMcpServer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_servers: Vec<String>,
    #[serde(default)]
    pub startup_incomplete: bool,
    #[serde(default)]
    pub startup_running: bool,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMcpServer {
    pub name: String,
    pub status: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_templates_summary: Option<String>,
}

static AGY_VERSION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bAntigravity(?:\s+CLI)?\s+([0-9]+(?:\.[0-9]+)+(?:[-+._A-Za-z0-9]*)?)")
        .expect("valid agy version regex")
});

static ACCOUNT_PLAN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"([A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})\s*\(([^)]+)\)")
        .expect("valid account plan regex")
});

static AGY_MODEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\b((?:Gemini|Claude|GPT(?:-OSS)?|OpenAI|Grok|Llama|Mistral|Qwen|DeepSeek)[A-Za-z0-9 ._/\-+]*(?:\([^)]+\))?)",
    )
    .expect("valid agy model regex")
});

static AGY_CWD_ONLY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?P<cwd>(?:~|/[A-Za-z0-9_-]+)/(?:[^\s|]+/?)+)$")
        .expect("valid agy cwd-only regex")
});

static AGY_CWD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?P<cwd>(?:~|/[A-Za-z0-9_-]+)/(?:[^\s|]+/?)+)").expect("valid agy cwd regex")
});

static PERCENT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([0-9]{1,3})%").expect("valid percent regex"));

static VISIBLE_RANGE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\((\d+)\s*[–-]\s*(\d+)\s+of\s+(\d+)\s+lines\)").expect("valid visible range regex")
});

static SHELL_PROMPT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:\([^)]+\)\s*)?[A-Za-z0-9._-]+@[^%\n$#]+[%$#]\s*$")
        .expect("valid shell prompt regex")
});

static CODEX_VERSION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"OpenAI\s+Codex\s+\(v(?P<version>[^)]+)\)").expect("valid codex version regex")
});

static CLAUDE_CODE_VERSION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Claude\s+Code\s+v(?P<version>[0-9]+(?:\.[0-9]+)+(?:[-+._A-Za-z0-9]*)?)")
        .expect("valid Claude Code version regex")
});

static CLAUDE_CODE_MODEL_PLAN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?P<model>(?:Claude\s+)?(?:Opus|Sonnet|Haiku)\s+[0-9]+(?:\.[0-9]+)?)\b(?:\s+\([^)]+\))?\s+with\s+(?P<effort>[A-Za-z0-9_-]+)\s+effort\s*·\s*(?P<plan>[^│\n]+)",
    )
    .expect("valid Claude Code model/plan regex")
});

static CLAUDE_CODE_STARTUP_OPTION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?P<selected>[❯>])?\s*(?P<index>\d+)\.\s+(?P<label>.+?)\s*$")
        .expect("valid Claude Code startup option regex")
});

impl PtyRecognitionSnapshot {
    fn new(provider: CliEngine, state: PtyCanonicalState, confidence: f64, reason: &str) -> Self {
        Self {
            provider,
            state,
            confidence,
            reason: reason.to_string(),
            phase: None,
            active_tool: None,
            elapsed_secs: None,
            blocked_kind: None,
            screen_identity: None,
            screen_usage: None,
            screen_mcp: None,
            screen_signals: None,
            source: "screen_fallback".to_string(),
        }
    }

    fn with_phase(mut self, phase: impl Into<String>) -> Self {
        self.phase = Some(phase.into());
        self
    }

    fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = reason.into();
        self
    }

    fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.active_tool = Some(tool.into());
        self
    }

    fn with_elapsed(mut self, elapsed_secs: Option<u64>) -> Self {
        self.elapsed_secs = elapsed_secs;
        self
    }

    fn with_blocked_kind(mut self, kind: impl Into<String>) -> Self {
        self.blocked_kind = Some(kind.into());
        self
    }

    fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    fn with_screen_identity(mut self, identity: Option<ProviderScreenIdentity>) -> Self {
        self.screen_identity = identity;
        self
    }

    fn with_screen_usage(mut self, usage: Option<ProviderUsageScreen>) -> Self {
        self.screen_usage = usage;
        self
    }

    fn with_screen_mcp(mut self, mcp: Option<ProviderMcpScreen>) -> Self {
        self.screen_mcp = mcp;
        self
    }

    fn with_screen_signals(mut self, signals: Option<ProviderScreenSignals>) -> Self {
        self.screen_signals = signals;
        self
    }
}

pub fn session_state_snapshot(provider: CliEngine, state: SessionState) -> PtyRecognitionSnapshot {
    let canonical = match state {
        SessionState::Idle | SessionState::SlashMenu => PtyCanonicalState::Idle,
        SessionState::Confirming => PtyCanonicalState::Blocked,
        SessionState::Thinking | SessionState::Responding | SessionState::ToolRunning => {
            PtyCanonicalState::Running
        }
        SessionState::Exited => PtyCanonicalState::Complete,
        SessionState::Starting | SessionState::Error => PtyCanonicalState::Unknown,
    };
    PtyRecognitionSnapshot::new(
        provider,
        canonical,
        if canonical == PtyCanonicalState::Unknown {
            0.55
        } else {
            0.8
        },
        &format!("session_state:{state:?}"),
    )
    .with_source("session_state")
}

pub fn is_provider_unavailable_snapshot(snapshot: &PtyRecognitionSnapshot) -> bool {
    snapshot.state == PtyCanonicalState::Blocked
        && matches!(
            snapshot.blocked_kind.as_deref(),
            Some("auth_missing" | "auth_code_required" | "billing_or_account" | "usage_limit")
        )
}

pub fn recognize_screen(
    provider: CliEngine,
    lines: &[String],
    current_state: SessionState,
) -> PtyRecognitionSnapshot {
    recognize_screen_inner(provider, lines, None, current_state)
}

pub fn recognize_styled_screen(
    provider: CliEngine,
    screen: &StyledScreenSnapshot,
    current_state: SessionState,
) -> PtyRecognitionSnapshot {
    let lines = screen
        .lines
        .iter()
        .map(|line| line.text.clone())
        .collect::<Vec<_>>();
    recognize_screen_inner(provider, &lines, Some(screen), current_state)
}

fn recognize_screen_inner(
    provider: CliEngine,
    lines: &[String],
    styled_screen: Option<&StyledScreenSnapshot>,
    current_state: SessionState,
) -> PtyRecognitionSnapshot {
    let mut snapshot = match provider {
        CliEngine::Codex => recognize_codex_with_style(lines, styled_screen),
        CliEngine::Gemini => recognize_gemini(lines),
        CliEngine::Agy => recognize_agy(lines),
        CliEngine::ClaudeCode => recognize_claude_code(lines),
    };
    if snapshot.state == PtyCanonicalState::Unknown {
        let screen_identity = snapshot.screen_identity.clone();
        snapshot = session_state_snapshot(provider, current_state);
        if snapshot.screen_identity.is_none() {
            snapshot.screen_identity = screen_identity;
        }
    }
    fuse_with_session_state(provider, lines, styled_screen, current_state, snapshot)
}

/// Provider-aware state fusion: a `screen_fallback` Blocked snapshot that
/// contradicts an actively processing SessionState reflects stale terminal
/// text (a confirmation or model-picker line that the worker has already
/// resolved). When SessionState is Thinking/Responding/ToolRunning we either
/// promote a fused active-evidence snapshot from the same screen or fall back
/// to the SessionState baseline. Explicit `Confirming` SessionState always
/// preserves Blocked, so true approval gates are not misreported as Running.
fn fuse_with_session_state(
    provider: CliEngine,
    lines: &[String],
    styled_screen: Option<&StyledScreenSnapshot>,
    current_state: SessionState,
    snapshot: PtyRecognitionSnapshot,
) -> PtyRecognitionSnapshot {
    if matches!(current_state, SessionState::Exited | SessionState::Error) {
        if is_provider_unavailable_snapshot(&snapshot) {
            return snapshot.with_source("screen_final");
        }
        return session_state_snapshot(provider, current_state);
    }
    if current_state == SessionState::Confirming {
        return snapshot;
    }
    if snapshot.state == PtyCanonicalState::Blocked
        && snapshot.source == "screen_fallback"
        && current_state.is_processing()
    {
        if let Some(active) = active_running_evidence(provider, lines, styled_screen) {
            return active;
        }
        return session_state_snapshot(provider, current_state);
    }
    snapshot
}

fn active_running_evidence(
    provider: CliEngine,
    lines: &[String],
    styled_screen: Option<&StyledScreenSnapshot>,
) -> Option<PtyRecognitionSnapshot> {
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let elapsed = extract_elapsed_secs(&text);
    let current_activity = has_current_claude_activity_line(lines);
    match provider {
        CliEngine::ClaudeCode => {
            if lower.contains("esc to interrupt")
                || lower.contains("almost done thinking")
                || lower.contains("thinking with")
                || current_activity
                || has_active_claude_spinner(lines)
            {
                let mut snapshot = PtyRecognitionSnapshot::new(
                    CliEngine::ClaudeCode,
                    PtyCanonicalState::Running,
                    0.9,
                    "claude_code:active_spinner",
                )
                .with_elapsed(elapsed)
                .with_screen_identity(extract_claude_code_screen_identity(lines))
                .with_source("screen_fused");
                snapshot = if let Some(tool) = extract_tool_name(lines) {
                    snapshot.with_tool(tool).with_phase("tool")
                } else if is_claude_code_logout_command_visible(lines) {
                    snapshot
                        .with_reason("claude_code:logout_running")
                        .with_phase("logout")
                } else {
                    snapshot.with_phase("thinking")
                };
                Some(snapshot)
            } else {
                None
            }
        }
        CliEngine::Codex => {
            if has_codex_current_running_status(lines, &lower, styled_screen) {
                let signals = extract_codex_screen_signals_with_style(lines, styled_screen);
                let mut snapshot = PtyRecognitionSnapshot::new(
                    CliEngine::Codex,
                    PtyCanonicalState::Running,
                    0.9,
                    "codex:status_indicator_widget",
                )
                .with_elapsed(elapsed)
                .with_source("screen_fused")
                .with_screen_identity(extract_codex_screen_identity(lines))
                .with_screen_signals(signals.clone());
                if let Some((phase, tool)) =
                    extract_codex_active_tool(lines, styled_screen, signals.as_ref())
                {
                    snapshot = snapshot.with_phase(phase).with_tool(tool);
                }
                Some(snapshot)
            } else {
                None
            }
        }
        CliEngine::Gemini => {
            if lower.contains("executing")
                || lower.contains("coretoolcallstatus.executing")
                || lower.contains("streamingstate.responding")
                || lower.contains("thinking...")
                || lower.contains("esc to cancel")
                || has_spinner(lines)
            {
                let snapshot = PtyRecognitionSnapshot::new(
                    CliEngine::Gemini,
                    PtyCanonicalState::Running,
                    0.9,
                    "gemini:loading_indicator_responding",
                )
                .with_phase("thinking")
                .with_elapsed(elapsed)
                .with_source("screen_fused");
                Some(snapshot)
            } else {
                None
            }
        }
        CliEngine::Agy => {
            if has_agy_active_status_line(lines) {
                let mut snapshot = PtyRecognitionSnapshot::new(
                    CliEngine::Agy,
                    PtyCanonicalState::Running,
                    0.88,
                    "agy:active_status",
                )
                .with_elapsed(elapsed)
                .with_source("screen_fused");
                if let Some(tool) = extract_tool_name(lines) {
                    snapshot = snapshot.with_tool(tool).with_phase("tool");
                } else {
                    snapshot = snapshot.with_phase("thinking");
                }
                Some(snapshot)
            } else {
                None
            }
        }
    }
}

fn recognize_codex(lines: &[String]) -> PtyRecognitionSnapshot {
    recognize_codex_with_style(lines, None)
}

fn recognize_codex_with_style(
    lines: &[String],
    styled_screen: Option<&StyledScreenSnapshot>,
) -> PtyRecognitionSnapshot {
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let elapsed = extract_elapsed_secs(&text);
    let identity = extract_codex_screen_identity(lines);
    let signals = extract_codex_screen_signals_with_style(lines, styled_screen);
    let mcp = extract_codex_mcp_screen(lines);

    if let Some((kind, reason)) = provider_unavailable_match(&lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Blocked,
            0.95,
            reason,
        )
        .with_blocked_kind(kind)
        .with_elapsed(elapsed)
        .with_source("provider_error_signature")
        .with_screen_identity(identity)
        .with_screen_signals(signals);
    }

    if is_codex_workspace_trust_prompt(lines, &lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Blocked,
            0.94,
            "codex:workspace_trust_prompt",
        )
        .with_blocked_kind("workspace_trust")
        .with_phase("startup_trust")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity)
        .with_screen_signals(signals);
    }

    if is_codex_permission_picker(lines, &lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Blocked,
            0.92,
            "codex:permission_picker",
        )
        .with_blocked_kind("permission_picker")
        .with_phase("permission_mode")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity)
        .with_screen_signals(signals);
    }

    if is_codex_approval_menu(&lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Blocked,
            0.96,
            "codex:mcp_approval_menu",
        )
        .with_blocked_kind("approval")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity)
        .with_screen_signals(signals);
    }

    if lower.contains("approval request")
        || lower.contains("request approval")
        || lower.contains("requires approval")
        || lower.contains("allow command")
        || lower.contains("file change request")
        || lower.contains("network exec prompt")
        || lower.contains("request_user_input")
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Blocked,
            0.94,
            "codex:approval_or_user_input",
        )
        .with_blocked_kind("approval")
        .with_elapsed(elapsed)
        .with_screen_identity(identity)
        .with_screen_signals(signals);
    }

    if is_codex_model_picker(lines, &lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Blocked,
            0.92,
            "codex:model_picker",
        )
        .with_blocked_kind("model_picker")
        .with_phase("model_switch")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity)
        .with_screen_signals(signals);
    }

    if let Some(mcp) = mcp.clone().filter(|mcp| mcp.startup_running) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Running,
            0.9,
            "codex:mcp_startup_running",
        )
        .with_phase("mcp_startup")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity)
        .with_screen_mcp(Some(mcp))
        .with_screen_signals(signals);
    }

    if has_codex_current_running_status(lines, &lower, styled_screen) {
        let mut snapshot = PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Running,
            0.9,
            "codex:status_indicator_widget",
        )
        .with_elapsed(elapsed)
        .with_screen_identity(identity)
        .with_screen_signals(signals.clone());
        if let Some((phase, tool)) =
            extract_codex_active_tool(lines, styled_screen, signals.as_ref())
        {
            snapshot = snapshot.with_phase(phase).with_tool(tool);
        }
        return snapshot;
    }

    if let Some(mcp) = mcp {
        let (state, reason) = if mcp.startup_incomplete {
            (PtyCanonicalState::Idle, "codex:mcp_startup_incomplete")
        } else {
            (PtyCanonicalState::Complete, "codex:mcp_inventory")
        };
        return PtyRecognitionSnapshot::new(CliEngine::Codex, state, 0.9, reason)
            .with_phase("mcp_status")
            .with_elapsed(elapsed)
            .with_source("tui_source_signature")
            .with_screen_identity(identity)
            .with_screen_mcp(Some(mcp))
            .with_screen_signals(signals);
    }

    if has_completion_line(lines) && has_idle_prompt(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Complete,
            0.86,
            "codex:turn_complete_prompt_returned",
        )
        .with_elapsed(elapsed)
        .with_screen_identity(identity)
        .with_screen_signals(signals);
    }

    if has_idle_prompt(lines)
        || lower.contains("ctrl+c to quit")
        || lower.contains("ctrl+c to interrupt")
        || lower.contains("footer_mode")
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Idle,
            0.88,
            "codex:composer_idle",
        )
        .with_screen_identity(identity)
        .with_screen_signals(signals);
    }

    PtyRecognitionSnapshot::new(
        CliEngine::Codex,
        PtyCanonicalState::Unknown,
        0.2,
        "codex:no_match",
    )
    .with_screen_identity(identity)
    .with_screen_signals(signals)
}

fn is_codex_approval_menu(lower: &str) -> bool {
    lower.contains("allow the ")
        && lower.contains(" mcp server to run tool")
        && lower.contains("allow for this session")
        && lower.contains("enter to submit")
        && lower.contains("esc to cancel")
}

fn is_codex_workspace_trust_prompt(_lines: &[String], lower: &str) -> bool {
    lower.contains("do you trust the contents of this directory")
        && lower.contains("yes, continue")
        && lower.contains("no, quit")
        && lower.contains("press enter to continue")
}

fn extract_codex_mcp_screen(lines: &[String]) -> Option<ProviderMcpScreen> {
    let mut failed_servers: Vec<String> = Vec::new();
    let mut startup_incomplete = false;
    let mut startup_running = false;
    for line in lines {
        let cleaned = normalize_identity_value(line);
        let lower = cleaned.to_ascii_lowercase();
        if lower.contains("starting mcp servers") {
            startup_running = true;
        }
        if lower.contains("mcp startup incomplete") {
            startup_incomplete = true;
            for name in extract_codex_failed_mcp_servers(&cleaned) {
                if !failed_servers
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(name.as_str()))
                {
                    failed_servers.push(name);
                }
            }
        }
    }

    let mut servers = Vec::new();
    let mut in_mcp_tools = false;
    let mut current: Option<ProviderMcpServer> = None;
    for line in lines {
        let cleaned = normalize_identity_value(line);
        let trimmed = cleaned.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("mcp tools") {
            if let Some(server) = current.take() {
                servers.push(finalize_codex_mcp_server(server, &failed_servers));
            }
            in_mcp_tools = true;
            servers.clear();
            continue;
        }
        if !in_mcp_tools {
            continue;
        }
        if trimmed.is_empty() || strip_codex_prompt_marker(trimmed).is_some() {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("• ") else {
            continue;
        };
        let rest = rest.trim();
        if let Some(value) = rest.strip_prefix("Auth:") {
            if let Some(server) = current.as_mut() {
                server.auth = Some(value.trim().to_string()).filter(|value| !value.is_empty());
            }
            continue;
        }
        if let Some(value) = rest.strip_prefix("Tools:") {
            if let Some(server) = current.as_mut() {
                let summary = value.trim().to_string();
                server.tools = codex_mcp_tools_from_summary(&summary);
                server.tools_summary = Some(summary).filter(|value| !value.is_empty());
            }
            continue;
        }
        if let Some(value) = rest.strip_prefix("Resources:") {
            if let Some(server) = current.as_mut() {
                server.resources_summary =
                    Some(value.trim().to_string()).filter(|value| !value.is_empty());
            }
            continue;
        }
        if let Some(value) = rest.strip_prefix("Resource templates:") {
            if let Some(server) = current.as_mut() {
                server.resource_templates_summary =
                    Some(value.trim().to_string()).filter(|value| !value.is_empty());
            }
            continue;
        }
        if let Some(server) = current.take() {
            servers.push(finalize_codex_mcp_server(server, &failed_servers));
        }
        current = Some(ProviderMcpServer {
            name: rest.to_string(),
            status: "unknown".to_string(),
            connected: false,
            ..ProviderMcpServer::default()
        });
    }
    if let Some(server) = current.take() {
        servers.push(finalize_codex_mcp_server(server, &failed_servers));
    }

    if !startup_running && !startup_incomplete && servers.is_empty() {
        return None;
    }

    let status = if startup_running {
        "starting"
    } else if startup_incomplete || servers.iter().any(|server| server.status == "failed") {
        "degraded"
    } else if servers.iter().any(|server| server.connected) {
        "connected"
    } else {
        "unknown"
    }
    .to_string();

    Some(ProviderMcpScreen {
        title: "MCP Tools".to_string(),
        status,
        servers,
        failed_servers,
        startup_incomplete,
        startup_running,
        verbose: lines.iter().any(|line| {
            let lower = normalize_identity_value(line).to_ascii_lowercase();
            lower.contains("resource templates:") || lower.contains("resources:")
        }),
    })
}

fn extract_codex_failed_mcp_servers(line: &str) -> Vec<String> {
    let lower = line.to_ascii_lowercase();
    let Some(start) = lower.find("failed:") else {
        return Vec::new();
    };
    let rest = &line[start + "failed:".len()..];
    let end = rest.find(')').unwrap_or(rest.len());
    rest[..end]
        .split(',')
        .map(|value| value.trim().trim_matches(&['`', '\'', '"'][..]))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn finalize_codex_mcp_server(
    mut server: ProviderMcpServer,
    failed_servers: &[String],
) -> ProviderMcpServer {
    let failed = failed_servers
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&server.name));
    let tools_none = server
        .tools_summary
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("(none)"));
    server.connected = !failed && !tools_none && server.tools_summary.is_some();
    server.status = if failed {
        "failed"
    } else if server.connected {
        "connected"
    } else {
        "unknown"
    }
    .to_string();
    server
}

fn codex_mcp_tools_from_summary(summary: &str) -> Vec<String> {
    if summary.trim().eq_ignore_ascii_case("(none)") {
        return Vec::new();
    }
    summary
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.starts_with('+'))
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexModelPickerRow {
    model: String,
    selected: bool,
    current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexPermissionPickerRow {
    mode: String,
    selected: bool,
}

fn is_codex_model_picker(lines: &[String], lower: &str) -> bool {
    (lower.contains("select model and effort")
        && lower.contains("press enter to confirm or esc to go back"))
        || (lower.contains("access legacy models by running codex -m")
            && codex_model_picker_rows(lines).len() >= 2)
}

fn is_codex_permission_picker(lines: &[String], lower: &str) -> bool {
    lower.contains("update model permissions")
        && lower.contains("press enter to confirm or esc to go back")
        && codex_permission_picker_rows(lines).len() >= 3
}

fn codex_model_picker_rows(lines: &[String]) -> Vec<CodexModelPickerRow> {
    lines
        .iter()
        .filter_map(|line| parse_codex_model_picker_row(line))
        .collect()
}

fn parse_codex_model_picker_row(line: &str) -> Option<CodexModelPickerRow> {
    let mut trimmed = line.trim_start();
    let selected = trimmed.starts_with('›') || trimmed.starts_with('>') || trimmed.starts_with('❯');
    if selected {
        trimmed = trimmed
            .trim_start_matches(|ch| matches!(ch, '›' | '>' | '❯'))
            .trim_start();
    }

    let (number, body) = trimmed.split_once('.')?;
    if number.trim().parse::<usize>().is_err() {
        return None;
    }

    let body = body.trim_start();
    let model_part = body
        .split("  ")
        .next()
        .unwrap_or(body)
        .replace("(current)", "");
    let model = normalize_model_value(&model_part);
    if !looks_like_codex_model(&model) {
        return None;
    }

    Some(CodexModelPickerRow {
        model,
        selected,
        current: body.contains("(current)"),
    })
}

fn codex_permission_picker_rows(lines: &[String]) -> Vec<CodexPermissionPickerRow> {
    lines
        .iter()
        .filter_map(|line| parse_codex_permission_picker_row(line))
        .collect()
}

fn parse_codex_permission_picker_row(line: &str) -> Option<CodexPermissionPickerRow> {
    let mut trimmed = line.trim_start();
    let selected = trimmed.starts_with('›') || trimmed.starts_with('>') || trimmed.starts_with('❯');
    if selected {
        trimmed = trimmed
            .trim_start_matches(|ch| matches!(ch, '›' | '>' | '❯'))
            .trim_start();
    }

    let (number, body) = trimmed.split_once('.')?;
    if number.trim().parse::<usize>().is_err() {
        return None;
    }

    let body = body.trim_start();
    let mode = normalize_codex_permission_mode(body)?;
    Some(CodexPermissionPickerRow { mode, selected })
}

fn normalize_codex_permission_mode(value: &str) -> Option<String> {
    let lower = normalize_identity_value(value).to_ascii_lowercase();
    if lower.starts_with("default") {
        Some("Default".to_string())
    } else if lower.starts_with("auto-review") || lower.starts_with("auto review") {
        Some("Auto-review".to_string())
    } else if lower.starts_with("full access") {
        Some("Full Access".to_string())
    } else {
        None
    }
}

fn extract_codex_screen_identity(lines: &[String]) -> Option<ProviderScreenIdentity> {
    let mut identity = ProviderScreenIdentity::default();
    for line in lines {
        let cleaned = normalize_identity_value(line);
        if let Some(caps) = CODEX_VERSION_RE.captures(&cleaned) {
            identity.cli_version = Some(caps["version"].trim().to_string());
            continue;
        }
        if let Some(rest) = cleaned.trim_start().strip_prefix("model:") {
            let model = rest
                .split("/model")
                .next()
                .unwrap_or(rest)
                .trim()
                .to_string();
            if !model.is_empty() {
                identity.current_model = Some(model);
            }
            continue;
        }
        if let Some(rest) = cleaned.trim_start().strip_prefix("directory:") {
            let cwd = rest.trim().to_string();
            if !cwd.is_empty() {
                identity.cwd = Some(cwd);
            }
            continue;
        }
        if cleaned.contains('·') {
            let parts = cleaned
                .split('·')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            if parts.len() >= 2 && looks_like_codex_model(parts[0]) && looks_like_cwd(parts[1]) {
                identity
                    .current_model
                    .get_or_insert_with(|| parts[0].to_string());
                identity.cwd.get_or_insert_with(|| parts[1].to_string());
            }
        }
    }
    let picker_rows = codex_model_picker_rows(lines);
    if !picker_rows.is_empty() {
        if let Some(row) = picker_rows.iter().find(|row| row.current) {
            identity.current_model = Some(row.model.clone());
        }
        if let Some(row) = picker_rows.iter().find(|row| row.selected) {
            identity.selected_model = Some(row.model.clone());
        }
    }
    if identity.is_empty() {
        None
    } else {
        Some(identity)
    }
}

fn extract_codex_screen_signals_with_style(
    lines: &[String],
    styled_screen: Option<&StyledScreenSnapshot>,
) -> Option<ProviderScreenSignals> {
    let mut signals = ProviderScreenSignals::default();
    let mut last_explored_idx: Option<usize> = None;
    let lower = joined_text(lines).to_ascii_lowercase();
    let model_picker_visible = is_codex_model_picker(lines, &lower);
    let permission_picker_visible = is_codex_permission_picker(lines, &lower);
    if model_picker_visible {
        signals.model_picker_visible = true;
        signals.visible_models = codex_model_picker_rows(lines)
            .into_iter()
            .map(|row| row.model)
            .collect();
    }
    if permission_picker_visible {
        signals.permission_picker_visible = true;
        let rows = codex_permission_picker_rows(lines);
        signals.visible_permission_modes = rows.iter().map(|row| row.mode.clone()).collect();
        signals.selected_permission_mode = rows
            .iter()
            .find(|row| row.selected)
            .map(|row| row.mode.clone());
    }

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if is_separator_line(trimmed) {
            signals.separator_count += 1;
        }
        if trimmed.contains("(ctrl + t to view transcript)") {
            signals.folded_tool_output = true;
        }

        if !model_picker_visible && !permission_picker_visible {
            if let Some(prompt_text) = strip_codex_prompt_marker(trimmed) {
                let styled_prompt = styled_screen
                    .and_then(|screen| screen.lines.get(idx))
                    .and_then(styled_codex_prompt_line);
                if styled_prompt
                    .as_ref()
                    .is_some_and(|prompt| prompt.is_placeholder)
                    || (styled_prompt.is_none() && is_codex_placeholder_text(prompt_text))
                {
                    let placeholder_text = styled_prompt
                        .map(|prompt| prompt.text)
                        .unwrap_or_else(|| prompt_text.to_string());
                    signals.placeholder_visible = true;
                    signals.placeholder_text = Some(placeholder_text);
                } else if !prompt_text.is_empty() {
                    let user_text = styled_prompt
                        .map(|prompt| prompt.text)
                        .unwrap_or_else(|| prompt_text.to_string());
                    if !user_text.is_empty() {
                        signals.last_user_message = Some(user_text);
                    }
                }
            }
        }

        if let Some(tool_label) = trimmed.strip_prefix("└ ") {
            if last_explored_idx.is_some() {
                signals.last_tool_kind = Some("explore".to_string());
                signals.last_tool_label = Some(tool_label.trim().to_string());
                last_explored_idx = None;
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("• ") {
            let lower = rest.to_ascii_lowercase();
            if let Some(command) = rest.strip_prefix("Ran ") {
                signals.last_tool_kind = Some("shell".to_string());
                signals.last_tool_label = Some(command.trim().to_string());
                last_explored_idx = None;
                continue;
            }
            if rest == "Explored" {
                signals.last_tool_kind = Some("explore".to_string());
                signals.last_tool_label = None;
                last_explored_idx = Some(idx);
                continue;
            }
            if lower == "searching the web" {
                signals.web_search_active = true;
                signals.last_tool_kind = Some("web_search".to_string());
                signals.last_tool_label = Some("Searching the web".to_string());
                last_explored_idx = None;
                continue;
            }
            if let Some(query) = rest.strip_prefix("Searched ") {
                signals.last_tool_kind = Some("web_search".to_string());
                signals.last_tool_label = Some(query.trim().to_string());
                last_explored_idx = None;
                continue;
            }
            if !rest.trim().is_empty() {
                signals.last_assistant_message = Some(rest.trim().to_string());
            }
        } else if trimmed == "◦ Searching the web" {
            signals.web_search_active = true;
            signals.last_tool_kind = Some("web_search".to_string());
            signals.last_tool_label = Some("Searching the web".to_string());
            last_explored_idx = None;
        }
    }

    if signals.is_empty() {
        None
    } else {
        Some(signals)
    }
}

fn has_codex_current_running_status(
    lines: &[String],
    lower: &str,
    styled_screen: Option<&StyledScreenSnapshot>,
) -> bool {
    if lower.contains("reviewing approval request") {
        return true;
    }
    for line in codex_recent_content_lines_with_style(lines, styled_screen) {
        if codex_line_is_active_status(line) {
            return true;
        }
        let trimmed = line.trim();
        if is_separator_line(trimmed)
            || (trimmed.starts_with("• ")
                && trimmed != "• Searching the web"
                && !trimmed.starts_with("• Ran ")
                && trimmed != "• Explored")
        {
            return false;
        }
    }
    false
}

fn extract_codex_active_tool(
    lines: &[String],
    styled_screen: Option<&StyledScreenSnapshot>,
    signals: Option<&ProviderScreenSignals>,
) -> Option<(&'static str, String)> {
    if codex_recent_content_lines_with_style(lines, styled_screen)
        .into_iter()
        .any(|line| {
            let lower = line.to_ascii_lowercase();
            lower == "◦ searching the web" || lower == "• searching the web"
        })
    {
        return Some(("web_search", "web_search".to_string()));
    }
    if let Some(signals) = signals {
        match signals.last_tool_kind.as_deref() {
            Some("shell") => {
                return Some((
                    "tool",
                    signals
                        .last_tool_label
                        .clone()
                        .unwrap_or_else(|| "shell".to_string()),
                ));
            }
            Some("explore") => {
                return Some((
                    "tool",
                    signals
                        .last_tool_label
                        .clone()
                        .unwrap_or_else(|| "explore".to_string()),
                ));
            }
            Some("web_search") if signals.web_search_active => {
                return Some(("web_search", "web_search".to_string()));
            }
            _ => {}
        }
    }
    extract_tool_name(lines).map(|tool| ("tool", tool))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexPromptLine {
    text: String,
    is_placeholder: bool,
}

fn styled_codex_prompt_line(line: &StyledScreenLine) -> Option<CodexPromptLine> {
    let marker_index = line
        .text
        .chars()
        .position(|ch| !ch.is_whitespace())
        .filter(|idx| {
            line.text
                .chars()
                .nth(*idx)
                .is_some_and(|ch| ch == '›' || ch == '>')
        })?;

    let text = line
        .text
        .chars()
        .skip(marker_index + 1)
        .collect::<String>()
        .trim()
        .to_string();
    if text.is_empty() {
        return Some(CodexPromptLine {
            text,
            is_placeholder: false,
        });
    }

    let mut cell_index = 0usize;
    let mut saw_placeholder_style = false;
    let mut saw_non_placeholder_style = false;
    for span in &line.spans {
        for ch in span.text.chars() {
            if cell_index > marker_index && !ch.is_whitespace() {
                if codex_span_is_placeholder_style(span) {
                    saw_placeholder_style = true;
                } else {
                    saw_non_placeholder_style = true;
                }
            }
            cell_index += 1;
        }
    }

    Some(CodexPromptLine {
        text,
        is_placeholder: saw_placeholder_style && !saw_non_placeholder_style,
    })
}

fn codex_span_is_placeholder_style(span: &StyledScreenSpan) -> bool {
    span.flags.dim && !span.flags.bold && !span.flags.inverse && !span.flags.hidden
}

fn strip_codex_prompt_marker(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    trimmed
        .strip_prefix('›')
        .or_else(|| trimmed.strip_prefix('>'))
        .map(str::trim)
}

fn codex_recent_content_lines_with_style<'a>(
    lines: &'a [String],
    styled_screen: Option<&StyledScreenSnapshot>,
) -> Vec<&'a str> {
    lines
        .iter()
        .enumerate()
        .rev()
        .map(|(idx, line)| (idx, line.trim()))
        .filter(|(_, line)| !line.is_empty())
        .filter(|(idx, line)| {
            let is_styled_placeholder = styled_screen
                .and_then(|screen| screen.lines.get(*idx))
                .and_then(styled_codex_prompt_line)
                .is_some_and(|prompt| prompt.is_placeholder);
            !is_codex_footer_line(line)
                && !is_styled_placeholder
                && !strip_codex_prompt_marker(line).is_some_and(is_codex_placeholder_text)
        })
        .map(|(_, line)| line)
        .take(14)
        .collect()
}

fn codex_line_is_active_status(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("working (")
        || lower.contains(" esc to interrupt")
        || lower.contains("running command")
        || lower.contains("command running")
        || lower == "◦ searching the web"
        || lower == "• searching the web"
        || has_spinner_in_line(line)
}

fn is_codex_placeholder_text(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized == "find and fix a bug in @filename"
        || normalized == "find and fix a bug in filename"
}

fn is_codex_footer_line(value: &str) -> bool {
    let parts = value
        .split('·')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    parts.len() >= 2 && looks_like_codex_model(parts[0]) && looks_like_cwd(parts[1])
}

fn looks_like_codex_model(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("gpt-") || lower.starts_with("codex") || lower.contains(" xhigh")
}

fn looks_like_cwd(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("~/") || trimmed.starts_with('/')
}

fn recognize_gemini(lines: &[String]) -> PtyRecognitionSnapshot {
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let elapsed = extract_elapsed_secs(&text);

    if let Some((kind, reason)) = provider_unavailable_match(&lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Gemini,
            PtyCanonicalState::Blocked,
            0.95,
            reason,
        )
        .with_blocked_kind(kind)
        .with_elapsed(elapsed)
        .with_source("provider_error_signature");
    }

    if lower.contains("waiting_for_confirmation")
        || lower.contains("awaitingapproval")
        || lower.contains("confirming")
        || lower.contains("toolconfirmation")
        || lower.contains("waiting for confirmation")
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Gemini,
            PtyCanonicalState::Blocked,
            0.94,
            "gemini:waiting_for_confirmation",
        )
        .with_blocked_kind("tool_confirmation")
        .with_elapsed(elapsed);
    }

    if lower.contains("executing")
        || lower.contains("coretoolcallstatus.executing")
        || lower.contains("tool running")
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Gemini,
            PtyCanonicalState::Running,
            0.9,
            "gemini:tool_executing",
        )
        .with_phase("tool")
        .with_tool(extract_tool_name(lines).unwrap_or_else(|| "tool".to_string()))
        .with_elapsed(elapsed);
    }

    if lower.contains("streamingstate.responding")
        || lower.contains("thinking...")
        || lower.contains("esc to cancel")
        || has_spinner(lines)
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Gemini,
            PtyCanonicalState::Running,
            0.9,
            "gemini:loading_indicator_responding",
        )
        .with_phase("thinking")
        .with_elapsed(elapsed);
    }

    if lower.contains("streamingstate.idle")
        || lower.contains("type your message")
        || has_idle_prompt(lines)
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Gemini,
            PtyCanonicalState::Idle,
            0.9,
            "gemini:input_prompt_idle",
        );
    }

    PtyRecognitionSnapshot::new(
        CliEngine::Gemini,
        PtyCanonicalState::Unknown,
        0.2,
        "gemini:no_match",
    )
}

fn recognize_agy(lines: &[String]) -> PtyRecognitionSnapshot {
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let elapsed = extract_elapsed_secs(&text);
    let identity = extract_agy_screen_identity(lines);
    let usage = extract_agy_usage_screen(lines);

    if is_agy_shell_prompt_after_exit(lines, &lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Complete,
            0.94,
            "agy:shell_prompt_after_exit",
        )
        .with_phase("exited")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if is_agy_startup_signing_in(lines, &lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Running,
            0.9,
            "agy:startup_signing_in",
        )
        .with_phase("auth_signing_in")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if is_agy_oauth_authorization_prompt(lines, &lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.94,
            "agy:oauth_authorization_prompt",
        )
        .with_blocked_kind("auth_code_required")
        .with_phase("auth_oauth_code")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if lower.contains("press ctrl+d again to exit") {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.9,
            "agy:exit_confirm_pending",
        )
        .with_blocked_kind("exit_confirmation")
        .with_phase("exit_confirm")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if is_agy_login_method_prompt(lines, &lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.94,
            "agy:login_method_prompt",
        )
        .with_blocked_kind("auth_missing")
        .with_phase("auth_login_method")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if is_agy_workspace_trust_prompt(lines, &lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.92,
            "agy:workspace_trust_prompt",
        )
        .with_blocked_kind("workspace_trust")
        .with_phase("startup_trust")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if let Some((kind, reason)) = provider_unavailable_match(&lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.95,
            reason,
        )
        .with_blocked_kind(kind)
        .with_elapsed(elapsed)
        .with_source("provider_error_signature")
        .with_screen_identity(identity);
    }

    if lower.contains("how's the cli experience so far")
        || lower.contains("help us improve:")
        || lower.contains("[1] good  [2] fine  [3] bad  [0] skip")
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Complete,
            0.9,
            "agy:feedback_prompt_after_complete",
        )
        .with_elapsed(elapsed)
        .with_screen_identity(identity);
    }

    if usage.is_some() {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Complete,
            0.92,
            "agy:usage_meter",
        )
        .with_phase("usage_meter")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity)
        .with_screen_usage(usage);
    }

    if is_agy_mcp_servers_screen(lines, &lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.9,
            "agy:mcp_servers",
        )
        .with_blocked_kind("mcp_servers")
        .with_phase("mcp_status")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if is_agy_cli_help_output(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Complete,
            0.92,
            "agy:cli_help",
        )
        .with_phase("cli_help")
        .with_source("cli_help_signature")
        .with_screen_identity(identity);
    }

    if lower.contains("switch model")
        && (lower.contains("enter select") || lower.contains("navigate"))
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.9,
            "agy:model_picker",
        )
        .with_blocked_kind("model_picker")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if lower.contains("approval request")
        || lower.contains("allow command")
        || lower.contains("requires approval")
        || lower.contains("do you want to proceed")
        || lower.contains("do you want to allow")
        || lower.contains("confirm command")
        || lower.contains("confirm action")
        || lower.contains("mcp disconnected")
        || lower.contains("reconnect failed")
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.9,
            "agy:approval_or_mcp_recovery",
        )
        .with_blocked_kind("approval")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if lower.contains("file access")
        || lower.contains("allow access to this file")
        || lower.contains("reason: outside workspace")
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.9,
            "agy:file_access_approval",
        )
        .with_blocked_kind("approval")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if is_agy_slash_command_menu(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.9,
            "agy:slash_command_menu",
        )
        .with_blocked_kind("slash_command_menu")
        .with_phase("command_menu")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if is_agy_pending_slash_command(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Blocked,
            0.86,
            "agy:slash_command_pending",
        )
        .with_blocked_kind("slash_command_input")
        .with_phase("command_input")
        .with_elapsed(elapsed)
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if has_agy_active_status_line(lines) {
        let mut snapshot = PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Running,
            0.88,
            "agy:active_status",
        )
        .with_elapsed(elapsed);
        if let Some(tool) = extract_tool_name(lines) {
            snapshot = snapshot.with_tool(tool).with_phase("tool");
        } else {
            snapshot = snapshot.with_phase("thinking");
        }
        return snapshot.with_screen_identity(identity);
    }

    if lower.contains("interrupted")
        && lower.contains("what should antigravity cli do instead")
        && has_idle_prompt(lines)
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Idle,
            0.88,
            "agy:interrupted_ready_for_retry",
        )
        .with_phase("interrupted")
        .with_source("tui_source_signature")
        .with_screen_identity(identity);
    }

    if has_completion_line(lines) && has_idle_prompt(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Complete,
            0.84,
            "agy:turn_complete_prompt_returned",
        )
        .with_elapsed(elapsed)
        .with_screen_identity(identity);
    }

    if lower.contains("type your message")
        || lower.contains("welcome back")
        || lower.contains("bypass permissions on")
        || has_idle_prompt(lines)
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::Agy,
            PtyCanonicalState::Idle,
            0.86,
            "agy:composer_idle",
        )
        .with_screen_identity(identity);
    }

    PtyRecognitionSnapshot::new(
        CliEngine::Agy,
        PtyCanonicalState::Unknown,
        0.2,
        "agy:no_match",
    )
    .with_screen_identity(identity)
}

fn recognize_claude_code(lines: &[String]) -> PtyRecognitionSnapshot {
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let elapsed = extract_elapsed_secs(&text);
    let current_activity = has_current_claude_activity_line(lines);
    let identity = extract_claude_code_screen_identity(lines);
    let startup_signals = extract_claude_code_startup_config_signals(lines, &lower);
    let mcp = extract_claude_code_mcp_screen(lines);

    if let Some((kind, reason)) = provider_unavailable_match(&lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Blocked,
            0.95,
            reason,
        )
        .with_blocked_kind(kind)
        .with_elapsed(elapsed)
        .with_screen_identity(identity)
        .with_source("provider_error_signature");
    }

    if startup_signals.is_some() {
        let startup_kind = startup_signals
            .as_ref()
            .and_then(|signals| signals.startup_prompt_kind.as_deref())
            .unwrap_or("startup_config");
        let (reason, blocked_kind, phase) = match startup_kind {
            "oauth_authorization" => (
                "claude_code:oauth_authorization_prompt",
                "auth_code_required",
                "auth_oauth",
            ),
            "login_success_continue" => (
                "claude_code:login_success_continue",
                "startup_continue",
                "auth_login_success",
            ),
            "security_notes_continue" => (
                "claude_code:security_notes_continue",
                "startup_continue",
                "startup_security_notes",
            ),
            "login_method" => (
                "claude_code:login_method_prompt",
                "auth_missing",
                "auth_login_method",
            ),
            _ => (
                "claude_code:first_run_theme_prompt",
                "startup_config",
                "startup_theme",
            ),
        };
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Blocked,
            0.94,
            reason,
        )
        .with_blocked_kind(blocked_kind)
        .with_phase(phase)
        .with_elapsed(elapsed)
        .with_screen_identity(identity)
        .with_screen_signals(startup_signals)
        .with_source("tui_source_signature");
    }

    if let Some(mcp) = mcp {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Blocked,
            0.92,
            "claude_code:mcp_servers",
        )
        .with_blocked_kind("mcp_servers")
        .with_phase("mcp_status")
        .with_elapsed(elapsed)
        .with_screen_identity(identity)
        .with_screen_mcp(Some(mcp))
        .with_source("tui_source_signature");
    }

    // Only explicit confirmation / model-picker UI surfaces count. Generic
    // mentions of `approval` or `permission(s)` in the visible scroll buffer
    // (task brief prose, historical tool output, the "bypass permissions on"
    // composer-mode footer toggle) MUST NOT trigger Blocked: those words live
    // on idle and completed screens. We pin the upstream modal phrases from
    // src/components/permissions/* and ModelPicker.tsx instead.
    if lower.contains("enter to confirm")
        || lower.contains("do you want to proceed")
        || lower.contains("do you want to make this edit")
        || lower.contains("do you want to allow")
        || lower.contains("do you want to use this api key")
        || lower.contains("select model")
        || lower.contains("approval request")
    {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Blocked,
            0.9,
            "claude_code:confirmation_or_picker",
        )
        .with_blocked_kind("confirmation")
        .with_elapsed(elapsed)
        .with_screen_identity(identity);
    }

    if has_completion_line(lines) && has_idle_prompt(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Complete,
            0.86,
            "claude_code:turn_completion_verb",
        )
        .with_elapsed(elapsed)
        .with_screen_identity(identity);
    }

    if (lower.contains("auto mode on") || has_idle_prompt(lines)) && !current_activity {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Idle,
            0.9,
            "claude_code:prompt_idle",
        )
        .with_screen_identity(identity);
    }

    if lower.contains("esc to interrupt")
        || lower.contains("almost done thinking")
        || lower.contains("thinking with")
        || current_activity
        || has_active_claude_spinner(lines)
    {
        let mut snapshot = PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Running,
            0.9,
            "claude_code:active_spinner",
        )
        .with_elapsed(elapsed)
        .with_screen_identity(identity);
        if let Some(tool) = extract_tool_name(lines) {
            snapshot = snapshot.with_tool(tool).with_phase("tool");
        } else if is_claude_code_logout_command_visible(lines) {
            snapshot = snapshot
                .with_reason("claude_code:logout_running")
                .with_phase("logout");
        } else {
            snapshot = snapshot.with_phase("thinking");
        }
        return snapshot;
    }

    PtyRecognitionSnapshot::new(
        CliEngine::ClaudeCode,
        PtyCanonicalState::Unknown,
        0.2,
        "claude_code:no_match",
    )
    .with_screen_identity(identity)
}

fn extract_claude_code_mcp_screen(lines: &[String]) -> Option<ProviderMcpScreen> {
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let list_screen = lower.contains("manage mcp servers")
        && (lower.contains("code.claude.com/docs/en/mcp")
            || lower.contains("↑/↓ to navigate")
            || lower.contains("mcp servers"));
    let detail_screen = lower.contains("mcp server")
        && lower.contains("status:")
        && lower.contains("config location:")
        && lower.contains("reconnect");
    if !list_screen && !detail_screen {
        return None;
    }

    let mut servers = Vec::new();
    let mut failed_servers = Vec::new();
    let title = if detail_screen {
        lines
            .iter()
            .map(|line| normalize_identity_value(line))
            .find(|line| line.to_ascii_lowercase().ends_with(" mcp server"))
            .unwrap_or_else(|| "MCP Server".to_string())
    } else {
        "Manage MCP servers".to_string()
    };

    if detail_screen {
        let name = title
            .strip_suffix(" MCP Server")
            .unwrap_or(title.as_str())
            .trim()
            .to_string();
        let status = lines
            .iter()
            .filter_map(|line| {
                normalize_identity_value(line)
                    .strip_prefix("Status:")
                    .map(|value| claude_code_mcp_status_from_marker(value.trim()).0)
            })
            .next()
            .unwrap_or_else(|| "unknown".to_string());
        let connected = status == "connected";
        if status == "failed" {
            failed_servers.push(name.clone());
        }
        servers.push(ProviderMcpServer {
            name,
            status,
            connected,
            ..ProviderMcpServer::default()
        });
    } else {
        for line in lines {
            let cleaned = normalize_identity_value(line);
            let trimmed = strip_claude_code_mcp_selection(&cleaned);
            let Some(server) = parse_claude_code_mcp_server_row(trimmed) else {
                continue;
            };
            if server.status == "failed"
                && !failed_servers
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&server.name))
            {
                failed_servers.push(server.name.clone());
            }
            servers.push(server);
        }
    }

    let status = if servers.is_empty() {
        "unknown"
    } else if servers.iter().any(|server| server.status == "failed") {
        "degraded"
    } else if servers
        .iter()
        .any(|server| matches!(server.status.as_str(), "needs_authentication" | "disabled"))
    {
        "degraded"
    } else if servers.iter().any(|server| server.connected) {
        "connected"
    } else {
        "unknown"
    }
    .to_string();

    Some(ProviderMcpScreen {
        title,
        status,
        servers,
        failed_servers,
        startup_incomplete: false,
        startup_running: false,
        verbose: detail_screen,
    })
}

fn strip_claude_code_mcp_selection(line: &str) -> &str {
    line.trim_start()
        .trim_start_matches(|c: char| matches!(c, '❯' | '>' | '›' | '●'))
        .trim_start()
}

fn parse_claude_code_mcp_server_row(line: &str) -> Option<ProviderMcpServer> {
    let parts = line.split('·').map(str::trim).collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    let name = parts.first()?.trim();
    let lower_name = name.to_ascii_lowercase();
    if name.is_empty()
        || name.starts_with('↑')
        || name.starts_with('↓')
        || name.starts_with('⚠')
        || name.eq_ignore_ascii_case("claude.ai")
        || name.ends_with("MCPs")
        || name.contains("MCPs (")
        || lower_name.contains("setup issue")
    {
        return None;
    }

    let marker = parts.get(1).copied().unwrap_or_default();
    let (status, auth) = claude_code_mcp_status_from_marker(marker);
    let connected = status == "connected";
    Some(ProviderMcpServer {
        name: name.to_string(),
        status,
        connected,
        auth,
        tools_summary: parts.get(2).map(|value| value.to_string()),
        ..ProviderMcpServer::default()
    })
}

fn claude_code_mcp_status_from_marker(marker: &str) -> (String, Option<String>) {
    let lower = marker.to_ascii_lowercase();
    if lower.contains("connected") || marker.contains('✔') || marker.contains('✓') {
        ("connected".to_string(), None)
    } else if lower.contains("failed") || marker.contains('✘') || marker.contains('✗') {
        ("failed".to_string(), None)
    } else if lower.contains("needs authentication") || marker.contains('△') {
        (
            "needs_authentication".to_string(),
            Some("needs authentication".to_string()),
        )
    } else if lower.contains("disabled") || marker.contains('◯') {
        ("disabled".to_string(), None)
    } else {
        ("unknown".to_string(), None)
    }
}

fn extract_claude_code_screen_identity(lines: &[String]) -> Option<ProviderScreenIdentity> {
    let mut identity = ProviderScreenIdentity::default();
    identity.permission_mode = extract_claude_code_permission_mode(lines);
    for line in lines {
        let cleaned = normalize_identity_value(line);
        if let Some(captures) = CLAUDE_CODE_VERSION_RE.captures(&cleaned) {
            identity.cli_version = captures
                .name("version")
                .map(|value| normalize_identity_value(value.as_str()));
            continue;
        }
        if let Some(captures) = CLAUDE_CODE_MODEL_PLAN_RE.captures(&cleaned) {
            identity.current_model = captures
                .name("model")
                .map(|value| normalize_model_value(value.as_str()));
            identity.reasoning_effort = captures
                .name("effort")
                .map(|value| normalize_identity_value(value.as_str()));
            identity.plan = captures
                .name("plan")
                .map(|value| normalize_identity_value(value.as_str()));
            continue;
        }
        if identity.account.is_none() {
            if let Some(account) = cleaned
                .strip_prefix("Logged in as ")
                .map(normalize_identity_value)
                .filter(|value| value.contains('@'))
            {
                identity.account = Some(account);
                continue;
            }
        }
        if identity.cwd.is_none() && !cleaned.contains("://") {
            if let Some(captures) = AGY_CWD_RE.captures(&cleaned) {
                identity.cwd = captures
                    .name("cwd")
                    .and_then(|value| normalize_agy_cwd(value.as_str()));
            }
        }
    }

    if identity.is_empty() {
        None
    } else {
        Some(identity)
    }
}

fn extract_claude_code_permission_mode(lines: &[String]) -> Option<String> {
    lines.iter().rev().find_map(|line| {
        let cleaned = normalize_identity_value(line);
        let lower = cleaned.to_ascii_lowercase();
        let looks_like_footer = lower.contains("shift+tab to cycle")
            || lower.contains("? for shortcuts")
            || lower.contains("← for agents");
        if !looks_like_footer {
            return None;
        }
        if lower.contains("bypass permissions on") {
            return Some("bypass_permissions".to_string());
        }
        if lower.contains("auto mode on") {
            return Some("auto".to_string());
        }
        if lower.contains("accept edits on") {
            return Some("accept_edits".to_string());
        }
        if lower.contains("plan mode on") {
            return Some("plan".to_string());
        }
        if lower.contains("? for shortcuts") {
            return Some("default".to_string());
        }
        None
    })
}

fn extract_claude_code_startup_config_signals(
    lines: &[String],
    lower: &str,
) -> Option<ProviderScreenSignals> {
    let theme_prompt_visible = lower.contains("welcome to claude code")
        && lower.contains("choose the text style")
        && lower.contains("terminal");
    let login_method_visible = lower.contains("welcome to claude code")
        && lower.contains("select login method")
        && lower.contains("claude account with subscription")
        && lower.contains("anthropic console account")
        && lower.contains("3rd-party platform");
    let oauth_authorization_visible = lower.contains("browser didn't open?")
        && lower.contains("use the url below to sign in")
        && lower.contains("paste code here if prompted");
    let login_success_continue_visible = lower.contains("logged in as ")
        && lower.contains("login successful")
        && lower.contains("press enter to continue");
    let security_notes_continue_visible = lower.contains("security notes:")
        && lower.contains("claude can make mistakes")
        && lower.contains("prompt injection")
        && lower.contains("press enter to continue");
    let startup_prompt_kind = if theme_prompt_visible {
        "theme_picker"
    } else if login_method_visible {
        "login_method"
    } else if oauth_authorization_visible {
        "oauth_authorization"
    } else if login_success_continue_visible {
        "login_success_continue"
    } else if security_notes_continue_visible {
        "security_notes_continue"
    } else {
        return None;
    };

    let mut signals = ProviderScreenSignals {
        startup_prompt_visible: true,
        startup_prompt_kind: Some(startup_prompt_kind.to_string()),
        ..ProviderScreenSignals::default()
    };

    for line in lines {
        let cleaned = normalize_identity_value(line);
        let trimmed = cleaned.trim();
        let Some(captures) = CLAUDE_CODE_STARTUP_OPTION_RE.captures(trimmed) else {
            continue;
        };
        let Some(index) = captures
            .name("index")
            .and_then(|value| value.as_str().parse::<u16>().ok())
        else {
            continue;
        };
        let selected = captures.name("selected").is_some();
        let mut label = captures
            .name("label")
            .map(|value| value.as_str().trim().to_string())
            .unwrap_or_default();
        let checked = label.ends_with('✔');
        if checked {
            label = label.trim_end_matches('✔').trim().to_string();
        }
        if label.is_empty() {
            continue;
        }
        signals.visible_startup_options.push(label.clone());
        if selected {
            signals.selected_startup_option = Some(label);
            signals.selected_startup_option_index = Some(index);
            signals.selected_startup_option_checked = checked;
        }
    }

    Some(signals)
}

fn extract_agy_screen_identity(lines: &[String]) -> Option<ProviderScreenIdentity> {
    let text = joined_text(lines);
    let mut identity = ProviderScreenIdentity {
        cli_version: AGY_VERSION_RE
            .captures(&text)
            .and_then(|captures| captures.get(1))
            .map(|value| normalize_identity_value(value.as_str())),
        account: None,
        plan: None,
        current_model: extract_agy_current_model(lines),
        reasoning_effort: None,
        permission_mode: None,
        selected_model: extract_agy_selected_model(lines),
        cwd: extract_agy_cwd(lines),
    };

    if let Some(captures) = ACCOUNT_PLAN_RE.captures(&text) {
        identity.account = captures
            .get(1)
            .map(|value| normalize_identity_value(value.as_str()));
        identity.plan = captures
            .get(2)
            .map(|value| normalize_identity_value(value.as_str()));
    }

    if identity.is_empty() {
        None
    } else {
        Some(identity)
    }
}

fn extract_agy_usage_screen(lines: &[String]) -> Option<ProviderUsageScreen> {
    let quota_start = lines.iter().position(|line| {
        normalize_identity_value(&clean_agy_identity_line(line))
            .to_ascii_lowercase()
            .contains("model quota")
    })?;

    let visible_range = lines
        .iter()
        .filter_map(|line| parse_visible_range(&clean_agy_identity_line(line)))
        .next();

    let mut model_quotas = Vec::new();
    let mut index = quota_start + 1;
    while index < lines.len() {
        let cleaned = normalize_identity_value(&clean_agy_identity_line(&lines[index]));
        let lower = cleaned.to_ascii_lowercase();
        if cleaned.is_empty() {
            index += 1;
            continue;
        }
        if lower.contains("scroll")
            || lower.contains("pgup/pgdown")
            || lower.contains("ctrl+end")
            || lower.contains("ctrl+home")
            || lower == "close"
            || lower.contains("esc to cancel")
            || parse_visible_range(&cleaned).is_some()
        {
            break;
        }

        if let Some(model) = extract_agy_model_from_line(&cleaned) {
            let mut percent = None;
            let mut status = None;
            let mut lookahead = index + 1;
            while lookahead < lines.len() && lookahead <= index + 5 {
                let next = normalize_identity_value(&clean_agy_identity_line(&lines[lookahead]));
                let next_lower = next.to_ascii_lowercase();
                if next.is_empty() {
                    lookahead += 1;
                    continue;
                }
                if next_lower.contains("scroll")
                    || next_lower.contains("pgup/pgdown")
                    || next_lower.contains("ctrl+end")
                    || next_lower.contains("ctrl+home")
                    || next_lower == "close"
                    || next_lower.contains("esc to cancel")
                    || parse_visible_range(&next).is_some()
                    || extract_agy_model_from_line(&next).is_some()
                {
                    break;
                }
                let next_percent = parse_percent(&next);
                if percent.is_none() {
                    percent = next_percent;
                }
                if status.is_none() && !is_agy_meter_bar(&next) && next_percent.is_none() {
                    status = Some(next);
                    lookahead += 1;
                    break;
                }
                lookahead += 1;
            }
            model_quotas.push(ProviderModelQuota {
                model,
                percent,
                status,
            });
            index = lookahead;
            continue;
        }

        index += 1;
    }

    if model_quotas.is_empty() {
        None
    } else {
        Some(ProviderUsageScreen {
            title: "Model Quota".to_string(),
            model_quotas,
            visible_range,
        })
    }
}

fn parse_percent(line: &str) -> Option<u8> {
    let raw = PERCENT_RE
        .captures(line)
        .and_then(|captures| captures.get(1))?
        .as_str()
        .parse::<u16>()
        .ok()?;
    Some(raw.min(100) as u8)
}

fn parse_visible_range(line: &str) -> Option<ProviderVisibleRange> {
    let captures = VISIBLE_RANGE_RE.captures(line)?;
    Some(ProviderVisibleRange {
        start: captures.get(1)?.as_str().parse().ok()?,
        end: captures.get(2)?.as_str().parse().ok()?,
        total: captures.get(3)?.as_str().parse().ok()?,
    })
}

fn is_agy_meter_bar(line: &str) -> bool {
    line.chars()
        .filter(|value| !value.is_whitespace())
        .all(|value| {
            matches!(
                value,
                '█' | '▉' | '▊' | '▋' | '▌' | '▍' | '▎' | '▏' | '░' | '▒' | '▓'
            )
        })
}

fn extract_agy_current_model(lines: &[String]) -> Option<String> {
    for line in lines {
        if line.to_ascii_lowercase().contains("(current)") {
            if let Some(model) = extract_agy_model_from_line(line) {
                return Some(model);
            }
        }
    }

    let is_model_picker = lines
        .iter()
        .any(|line| line.to_ascii_lowercase().contains("switch model"));
    if is_model_picker {
        for line in lines.iter().rev() {
            if line.to_ascii_lowercase().contains("esc to cancel") {
                if let Some(model) = extract_agy_model_from_line(line) {
                    return Some(model);
                }
            }
        }
        return None;
    }

    lines
        .iter()
        .rev()
        .find_map(|line| extract_agy_model_from_line(line))
}

fn extract_agy_selected_model(lines: &[String]) -> Option<String> {
    let is_model_picker = lines
        .iter()
        .any(|line| line.to_ascii_lowercase().contains("switch model"));
    if !is_model_picker {
        return None;
    }

    for line in lines {
        if agy_line_has_selection_cursor(line) {
            if let Some(model) = extract_agy_model_from_line(line) {
                return Some(model);
            }
        }
    }
    None
}

fn extract_agy_model_from_line(line: &str) -> Option<String> {
    let cleaned = clean_agy_identity_line(line)
        .replace("(current)", "")
        .replace("[current]", "");
    let normalized = normalize_identity_value(&cleaned);
    if normalized.eq_ignore_ascii_case("switch model")
        || normalized.eq_ignore_ascii_case("keyboard:")
        || normalized
            .to_ascii_lowercase()
            .contains("navigate enter select")
    {
        return None;
    }

    AGY_MODEL_RE
        .captures(&normalized)
        .and_then(|captures| captures.get(1))
        .map(|value| normalize_model_value(value.as_str()))
        .filter(|value| !value.is_empty())
}

fn extract_agy_cwd(lines: &[String]) -> Option<String> {
    for line in lines {
        let cleaned = clean_agy_identity_line(line);
        if cleaned.contains("://") || is_probable_wrapped_url_path(&cleaned) {
            continue;
        }
        if let Some(captures) = AGY_CWD_ONLY_RE.captures(&cleaned) {
            return captures
                .name("cwd")
                .and_then(|value| normalize_agy_cwd(value.as_str()));
        }
    }

    for line in lines {
        let cleaned = clean_agy_identity_line(line);
        if cleaned.contains("://") || is_probable_wrapped_url_path(&cleaned) {
            continue;
        }
        let lower = cleaned.to_ascii_lowercase();
        if !(lower.contains("cwd") || lower.contains("directory") || lower.contains("project")) {
            continue;
        }
        if let Some(captures) = AGY_CWD_RE.captures(&cleaned) {
            return captures
                .name("cwd")
                .and_then(|value| normalize_agy_cwd(value.as_str()));
        }
    }

    None
}

fn normalize_agy_cwd(value: &str) -> Option<String> {
    let cwd = normalize_identity_value(value);
    if is_probable_wrapped_url_path(&cwd) {
        None
    } else {
        Some(cwd)
    }
}

fn is_probable_wrapped_url_path(cwd: &str) -> bool {
    let Some(rest) = cwd.strip_prefix('/') else {
        return false;
    };
    let first = rest.split('/').next().unwrap_or_default();
    first.contains('.')
}

fn clean_agy_identity_line(line: &str) -> String {
    line.trim()
        .trim_matches(|c: char| matches!(c, '│' | '┃' | '║' | '┆' | '┊'))
        .trim_start_matches(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '>' | '›' | '❯' | '|' | ':' | '-' | '*' | '·' | '╭' | '╰' | '╮' | '╯' | '─'
                )
        })
        .trim()
        .to_string()
}

fn agy_line_has_selection_cursor(line: &str) -> bool {
    let trimmed = line.trim_start().trim_start_matches(|c: char| {
        matches!(c, '│' | '┃' | '║' | '┆' | '┊') || c.is_whitespace()
    });
    trimmed.starts_with("> ") || trimmed.starts_with("› ") || trimmed.starts_with("❯ ")
}

fn normalize_identity_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_model_value(value: &str) -> String {
    normalize_identity_value(value)
        .trim_end_matches('-')
        .trim()
        .to_string()
}

fn provider_unavailable_match(lower: &str) -> Option<(&'static str, &'static str)> {
    if contains_any(
        lower,
        &[
            "credentials file not found",
            "may require interactive login",
            "not logged in",
            "login required",
            "please log in",
            "run /login",
            "authentication required",
            "invalid api key",
            "api key required",
            "invalid credentials",
            "no auth credentials",
            "unauthorized",
        ],
    ) {
        return Some(("auth_missing", "provider:auth_missing"));
    }
    if contains_any(
        lower,
        &[
            "account has been paused",
            "account is paused",
            "account suspended",
            "subscription is inactive",
            "subscription paused",
            "payment failed",
            "payment required",
            "update your billing",
            "billing issue",
            "billing problem",
            "organization has been disabled",
            "api access disabled",
            "claude code subscription",
        ],
    ) {
        return Some(("billing_or_account", "provider:billing_or_account"));
    }
    if contains_any(
        lower,
        &[
            "usage limit",
            "usage exceeded",
            "quota exceeded",
            "exhausted your daily quota",
            "terminalquotaerror",
            "rate limit exceeded",
            "too many requests",
        ],
    ) {
        return Some(("usage_limit", "provider:usage_limit"));
    }
    None
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

pub struct CodexCliStateParser {
    meta: ParserMeta,
}

impl CodexCliStateParser {
    pub fn new() -> Self {
        Self {
            meta: ParserMeta {
                name: "codex-cli-upstream".to_string(),
                description: "Codex CLI PTY parser derived from upstream TUI status surfaces"
                    .to_string(),
                priority: 20,
                version: "1.0.0".to_string(),
            },
        }
    }
}

impl StateParser for CodexCliStateParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn detect_state(&self, context: &ParserContext) -> Option<StateDetectionResult> {
        snapshot_to_detection(recognize_codex(&context.last_lines))
    }
}

pub struct GeminiCliUpstreamStateParser {
    meta: ParserMeta,
}

impl GeminiCliUpstreamStateParser {
    pub fn new() -> Self {
        Self {
            meta: ParserMeta {
                name: "gemini-cli-upstream".to_string(),
                description: "Gemini CLI PTY parser derived from upstream StreamingState surfaces"
                    .to_string(),
                priority: 20,
                version: "1.0.0".to_string(),
            },
        }
    }
}

impl StateParser for GeminiCliUpstreamStateParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn detect_state(&self, context: &ParserContext) -> Option<StateDetectionResult> {
        snapshot_to_detection(recognize_gemini(&context.last_lines))
    }
}

pub struct AgyCliStateParser {
    meta: ParserMeta,
}

impl AgyCliStateParser {
    pub fn new() -> Self {
        Self {
            meta: ParserMeta {
                name: "agy-cli".to_string(),
                description: "Antigravity CLI PTY parser derived from its interactive TUI surfaces"
                    .to_string(),
                priority: 20,
                version: "1.0.0".to_string(),
            },
        }
    }
}

impl StateParser for AgyCliStateParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn detect_state(&self, context: &ParserContext) -> Option<StateDetectionResult> {
        snapshot_to_detection(recognize_agy(&context.last_lines))
    }
}

fn snapshot_to_detection(snapshot: PtyRecognitionSnapshot) -> Option<StateDetectionResult> {
    if matches!(snapshot.provider, CliEngine::Agy | CliEngine::Codex)
        && matches!(
            snapshot.blocked_kind.as_deref(),
            Some(
                "model_picker"
                    | "permission_picker"
                    | "slash_command_menu"
                    | "slash_command_input"
                    | "mcp_servers"
            )
        )
    {
        return Some(StateDetectionResult::new(
            State::SlashMenu,
            snapshot.confidence,
        ));
    }

    let state = match snapshot.state {
        PtyCanonicalState::Idle | PtyCanonicalState::Complete => State::Idle,
        PtyCanonicalState::Blocked => State::Confirming,
        PtyCanonicalState::Running => {
            if snapshot.active_tool.is_some() || snapshot.phase.as_deref() == Some("tool") {
                State::ToolRunning
            } else if snapshot.reason.contains("responding") {
                State::Responding
            } else {
                State::Thinking
            }
        }
        PtyCanonicalState::Unknown => return None,
    };
    Some(StateDetectionResult::new(state, snapshot.confidence))
}

fn joined_text(lines: &[String]) -> String {
    lines.join("\n")
}

fn has_spinner(lines: &[String]) -> bool {
    lines.iter().any(|line| has_spinner_in_line(line))
}

fn has_spinner_in_line(line: &str) -> bool {
    line.chars().any(|c| {
        matches!(
            c,
            '\u{2800}'..='\u{28FF}' | '◐' | '◑' | '◒' | '◓' | '◴' | '◵' | '◶' | '◷'
        )
    })
}

fn has_agy_active_status_line(lines: &[String]) -> bool {
    let recent: Vec<_> = lines
        .iter()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(10)
        .collect();

    let has_cancel_footer = recent.iter().any(|line| {
        normalize_identity_value(&clean_agy_identity_line(line))
            .to_ascii_lowercase()
            .contains("esc to cancel")
    });
    let bottom_has_idle_footer = recent.iter().take(4).any(|line| {
        let lower = normalize_identity_value(&clean_agy_identity_line(line)).to_ascii_lowercase();
        lower.contains("? for shortcuts")
            || lower.contains("type your message")
            || lower.contains("welcome back")
    });

    if bottom_has_idle_footer && !has_cancel_footer {
        return false;
    }

    let has_status_spinner = recent.iter().any(|line| {
        let cleaned = normalize_identity_value(&clean_agy_identity_line(line));
        let lower = cleaned.to_ascii_lowercase();
        let status_text = lower.contains("generating")
            || lower.contains("working")
            || lower.contains("loading")
            || lower.contains("thinking...");
        status_text && has_spinner_in_line(line)
    });

    if has_status_spinner {
        return true;
    }

    has_cancel_footer
        && recent.iter().any(|line| {
            let cleaned = normalize_identity_value(&clean_agy_identity_line(line));
            let lower = cleaned.to_ascii_lowercase();
            lower.contains("running command") || lower.contains("tool running")
        })
}

fn is_agy_slash_command_menu(lines: &[String]) -> bool {
    let lower = joined_text(lines).to_ascii_lowercase();
    lower.contains("enter")
        && lower.contains("select")
        && lower.contains("tab")
        && lower.contains("complete")
        && lower.contains("esc to cancel")
        && lines.iter().any(|line| {
            let cleaned = normalize_identity_value(&clean_agy_identity_line(line));
            is_agy_composer_slash_input(&cleaned)
                || cleaned
                    .strip_prefix('>')
                    .is_some_and(is_agy_composer_slash_input)
                || cleaned
                    .strip_prefix('›')
                    .is_some_and(is_agy_composer_slash_input)
                || cleaned
                    .strip_prefix('❯')
                    .is_some_and(is_agy_composer_slash_input)
        })
}

fn is_agy_pending_slash_command(lines: &[String]) -> bool {
    let recent = lines
        .iter()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(8)
        .map(|line| normalize_identity_value(&clean_agy_identity_line(line)))
        .collect::<Vec<_>>();
    let has_ready_footer = recent
        .iter()
        .any(|line| line.to_ascii_lowercase().contains("? for shortcuts"));
    if !has_ready_footer {
        return false;
    }

    for line in recent {
        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("? for shortcuts") || is_separator_line(trimmed) {
            continue;
        }
        if matches!(trimmed, ">" | "›" | "❯") {
            return false;
        }
        if is_agy_composer_slash_input(trimmed)
            || trimmed
                .strip_prefix('>')
                .is_some_and(is_agy_composer_slash_input)
            || trimmed
                .strip_prefix('›')
                .is_some_and(is_agy_composer_slash_input)
            || trimmed
                .strip_prefix('❯')
                .is_some_and(is_agy_composer_slash_input)
        {
            return true;
        }
        if trimmed.starts_with("⎿") || trimmed.starts_with("└") || trimmed.starts_with("╰") {
            return false;
        }
    }
    false
}

fn is_agy_composer_slash_input(value: &str) -> bool {
    let trimmed = value.trim_start();
    let command = trimmed.strip_prefix('/').unwrap_or(trimmed);
    let Some(first) = command.chars().next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let head = command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches(':');
    !head.contains('/') && head.chars().all(|ch| ch.is_ascii_lowercase() || ch == '-')
}

fn is_agy_mcp_servers_screen(lines: &[String], lower: &str) -> bool {
    lower.contains("mcp servers")
        && lower.contains("keyboard:")
        && (lower.contains("enter actions") || lower.contains("enter select"))
        && lower.contains("esc to cancel")
        && lines.iter().any(|line| {
            let cleaned = normalize_identity_value(&clean_agy_identity_line(line));
            let lower = cleaned.to_ascii_lowercase();
            lower.starts_with("mcp servers")
                || lower.starts_with("plugins (")
                || lower.contains("tools:")
                || lower.contains("error:")
                || lower.contains("[restart]")
        })
}

fn is_separator_line(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '─' || ch == '-')
}

fn is_agy_cli_help_output(lines: &[String]) -> bool {
    let lower = joined_text(lines).to_ascii_lowercase();
    let has_usage = lines.iter().any(|line| {
        let line = normalize_identity_value(line).to_ascii_lowercase();
        line.starts_with("usage of ") && line.trim_end().ends_with("agy:")
    });
    has_usage
        && lower.contains("--dangerously-skip-permissions")
        && lower.contains("--print")
        && lower.contains("--prompt-interactive")
        && lower.contains("available subcommands:")
}

fn is_agy_shell_prompt_after_exit(lines: &[String], lower: &str) -> bool {
    if !lower.contains("resume with:") && !lower.contains("resume: agy --conversation=") {
        return false;
    }
    lines.iter().rev().take(8).any(|line| {
        SHELL_PROMPT_RE.is_match(&normalize_identity_value(&clean_agy_identity_line(line)))
    })
}

fn is_agy_startup_signing_in(lines: &[String], lower: &str) -> bool {
    lower.contains("welcome to the")
        && lower.contains("antigravity cli")
        && lower.contains("not signed in")
        && lower.contains("signing in")
        && lines.iter().any(|line| has_spinner_in_line(line))
}

fn is_agy_oauth_authorization_prompt(_lines: &[String], lower: &str) -> bool {
    lower.contains("open this link in the browser")
        && lower.contains("accounts.google.com/o/oauth2/auth")
        && lower.contains("paste the authorization code below")
        && lower.contains("authorization code")
}

fn is_agy_login_method_prompt(_lines: &[String], lower: &str) -> bool {
    lower.contains("welcome to the")
        && lower.contains("antigravity cli")
        && lower.contains("not signed in")
        && lower.contains("select login method")
        && lower.contains("google oauth")
        && lower.contains("use a google cloud project")
}

fn is_agy_workspace_trust_prompt(_lines: &[String], lower: &str) -> bool {
    lower.contains("accessing workspace")
        && lower.contains("do you trust the contents of this project")
        && lower.contains("yes, i trust this folder")
        && lower.contains("no, exit")
        && lower.contains("enter")
        && lower.contains("confirm")
}

fn is_claude_spinner_glyph(c: char) -> bool {
    "·✻✽✶✳✢*⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".contains(c)
}

fn strip_claude_spinner_prefix(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let first = trimmed.chars().next()?;
    if !is_claude_spinner_glyph(first) {
        return None;
    }
    Some(trimmed[first.len_utf8()..].trim_start())
}

fn is_claude_spinner_status_line(line: &str) -> bool {
    let Some(rest) = strip_claude_spinner_prefix(line) else {
        return false;
    };
    if !(rest.contains("...") || rest.contains('…')) {
        return false;
    }
    let lower = rest.to_ascii_lowercase();
    !lower.starts_with("idle") && !is_claude_completion_text(rest)
}

fn is_claude_explicit_active_spinner_line(line: &str) -> bool {
    if !is_claude_spinner_status_line(line) {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("esc to interrupt")
        || lower.contains("almost done thinking")
        || lower.contains("thinking with")
}

fn is_claude_user_prompt_with_input(line: &str) -> bool {
    let trimmed = line.trim();
    for marker in ["❯", "›", ">"] {
        let Some(rest) = trimmed.strip_prefix(marker) else {
            continue;
        };
        let rest = rest.trim();
        if rest.is_empty()
            || rest.contains("shift+tab to cycle")
            || rest.contains("? for shortcuts")
            || rest.contains("← for agents")
        {
            return false;
        }
        return true;
    }
    false
}

fn is_claude_code_logout_command_visible(lines: &[String]) -> bool {
    lines.iter().rev().take(20).any(|line| {
        let trimmed = line.trim();
        for marker in ["❯", "›", ">"] {
            let Some(rest) = trimmed.strip_prefix(marker) else {
                continue;
            };
            if rest.trim_start().starts_with("/logout") {
                return true;
            }
        }
        false
    })
}

fn is_claude_plain_idle_prompt(line: &str) -> bool {
    matches!(line.trim(), "❯" | "›" | ">")
}

fn has_active_claude_spinner(lines: &[String]) -> bool {
    lines.iter().any(|line| is_claude_spinner_status_line(line))
}

fn has_current_claude_activity_line(lines: &[String]) -> bool {
    let Some((spinner_idx, spinner_line)) = lines
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| is_claude_spinner_status_line(line))
    else {
        return false;
    };

    if is_claude_explicit_active_spinner_line(spinner_line) {
        return true;
    }

    if lines
        .iter()
        .skip(spinner_idx + 1)
        .any(|line| is_claude_completion_text(line))
    {
        return false;
    }

    let prompt_with_input_before_spinner = lines
        .iter()
        .take(spinner_idx)
        .rev()
        .take(12)
        .any(|line| is_claude_user_prompt_with_input(line));
    let has_active_footer = lines
        .iter()
        .rev()
        .take(8)
        .any(|line| line.to_ascii_lowercase().contains("esc to interrupt"));
    if prompt_with_input_before_spinner && has_active_footer {
        return true;
    }

    let non_empty_after_spinner = lines
        .iter()
        .skip(spinner_idx + 1)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let idle_prompt_after_spinner = non_empty_after_spinner
        .iter()
        .any(|line| is_claude_plain_idle_prompt(line));
    if idle_prompt_after_spinner {
        return false;
    }

    lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(6)
        .any(|(idx, _)| idx == spinner_idx)
}

fn has_idle_prompt(lines: &[String]) -> bool {
    lines
        .iter()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(4)
        .any(|line| {
            let trimmed = line.trim();
            trimmed == "❯"
                || trimmed == "›"
                || trimmed == ">"
                || trimmed == "$"
                || trimmed.starts_with("❯ ")
                || trimmed.starts_with("› ")
                || trimmed.starts_with("> ")
                || trimmed.starts_with("$ ")
                || trimmed.contains("│ >")
        })
}

fn has_completion_line(lines: &[String]) -> bool {
    lines
        .iter()
        .rev()
        .take(8)
        .any(|line| is_claude_completion_text(line))
}

fn is_claude_completion_text(line: &str) -> bool {
    let trimmed = line
        .trim_start()
        .trim_start_matches(|c: char| "·✻✽✶✳✢*⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ".contains(c));
    trimmed.starts_with("Worked for")
        || trimmed.starts_with("Baked for")
        || trimmed.starts_with("Brewed for")
        || trimmed.starts_with("Churned for")
        || trimmed.starts_with("Cogitated for")
        || trimmed.starts_with("Cooked for")
        || trimmed.starts_with("Crunched for")
        || trimmed.starts_with("Sautéed for")
}

fn extract_tool_name(lines: &[String]) -> Option<String> {
    for line in lines.iter().rev().take(16) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Bash(") {
            let end = rest.find(')').unwrap_or(rest.len());
            return Some(format!("Bash({})", &rest[..end]));
        }
        for prefix in [
            "Read ",
            "Write ",
            "Edit ",
            "MultiEdit ",
            "Grep ",
            "Glob ",
            "TodoWrite ",
        ] {
            if trimmed.starts_with(prefix) {
                return Some(prefix.trim().to_string());
            }
        }
        if let Some(tool) = trimmed
            .strip_prefix("Tool ")
            .and_then(|rest| rest.split_whitespace().next())
        {
            return Some(tool.to_string());
        }
    }
    None
}

fn extract_elapsed_secs(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let number: u64 = text[start..i].parse().ok()?;
            if i < bytes.len() {
                match bytes[i] {
                    b's' => return Some(number),
                    b'm' => {
                        let mut j = i + 1;
                        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        let sec_start = j;
                        while j < bytes.len() && bytes[j].is_ascii_digit() {
                            j += 1;
                        }
                        if j > sec_start && j < bytes.len() && bytes[j] == b's' {
                            let secs: u64 = text[sec_start..j].parse().ok()?;
                            return Some(number.saturating_mul(60).saturating_add(secs));
                        }
                        return Some(number.saturating_mul(60));
                    }
                    b'h' => return Some(number.saturating_mul(3600)),
                    _ => {}
                }
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screenshot::CapturedCellFlags;

    fn lines(input: &[&str]) -> Vec<String> {
        input.iter().map(|line| line.to_string()).collect()
    }

    fn styled_span(text: &str, dim: bool, bold: bool) -> StyledScreenSpan {
        StyledScreenSpan {
            text: text.to_string(),
            fg: [205, 214, 244],
            bg: [30, 30, 46],
            fg_hex: "#cdd6f4".to_string(),
            bg_hex: "#1e1e2e".to_string(),
            flags: CapturedCellFlags {
                bold,
                dim,
                ..CapturedCellFlags::default()
            },
        }
    }

    fn styled_line(spans: Vec<StyledScreenSpan>) -> StyledScreenLine {
        StyledScreenLine {
            text: spans.iter().map(|span| span.text.as_str()).collect(),
            spans,
        }
    }

    fn styled_screen(lines: Vec<StyledScreenLine>) -> StyledScreenSnapshot {
        StyledScreenSnapshot {
            rows: lines.len(),
            cols: 120,
            lines,
        }
    }

    #[test]
    fn codex_working_status_is_running() {
        let result = recognize_codex(&lines(&[
            "● Working (1m 02s • esc to interrupt) · running command",
            "  └ cargo test -p missiond-core",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Running);
        assert!(result.confidence >= 0.9);
        assert_eq!(result.elapsed_secs, Some(62));
    }

    #[test]
    fn codex_composer_prompt_is_idle() {
        let result = recognize_codex(&lines(&[">", "ctrl+c to quit"]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
    }

    #[test]
    fn codex_idle_screen_extracts_identity_and_placeholder() {
        let result = recognize_codex(&lines(&[
            "╭─────────────────────────────────────────────╮",
            "│ >_ OpenAI Codex (v0.135.0-alpha.1)          │",
            "│                                             │",
            "│ model:     gpt-5.5 xhigh   /model to change │",
            "│ directory: ~/Projects/missiond              │",
            "╰─────────────────────────────────────────────╯",
            "",
            "› Find and fix a bug in @filename",
            "",
            "  gpt-5.5 xhigh · ~/Projects/missiond",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
        let identity = result.screen_identity.expect("codex identity");
        assert_eq!(identity.cli_version.as_deref(), Some("0.135.0-alpha.1"));
        assert_eq!(identity.current_model.as_deref(), Some("gpt-5.5 xhigh"));
        assert_eq!(identity.cwd.as_deref(), Some("~/Projects/missiond"));
        let signals = result.screen_signals.expect("codex signals");
        assert!(signals.placeholder_visible);
        assert_eq!(
            signals.placeholder_text.as_deref(),
            Some("Find and fix a bug in @filename")
        );
    }

    #[test]
    fn codex_model_picker_tracks_current_selected_and_visible_models() {
        let result = recognize_codex(&lines(&[
            "Select Model and Effort",
            "Access legacy models by running codex -m <model_name> or in your config.toml",
            "",
            "› 1. gpt-5.5 (current)    Frontier model for coding tasks",
            "  2. gpt-5.4              Previous frontier model",
            "  3. gpt-5.4-mini         Smaller model",
            "  4. gpt-5.3-codex        Legacy coding model",
            "",
            "Press enter to confirm or esc to go back",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "codex:model_picker");
        assert_eq!(result.blocked_kind.as_deref(), Some("model_picker"));
        assert_eq!(result.phase.as_deref(), Some("model_switch"));

        let identity = result.screen_identity.expect("codex identity");
        assert_eq!(identity.current_model.as_deref(), Some("gpt-5.5"));
        assert_eq!(identity.selected_model.as_deref(), Some("gpt-5.5"));

        let signals = result.screen_signals.expect("codex signals");
        assert!(signals.model_picker_visible);
        assert_eq!(
            signals.visible_models,
            vec![
                "gpt-5.5".to_string(),
                "gpt-5.4".to_string(),
                "gpt-5.4-mini".to_string(),
                "gpt-5.3-codex".to_string()
            ]
        );
        assert!(signals.last_user_message.is_none());
    }

    #[test]
    fn codex_model_picker_maps_to_slash_menu_not_confirmation() {
        let result = snapshot_to_detection(recognize_codex(&lines(&[
            "Select Model and Effort",
            "› 1. gpt-5.5 (current)    Frontier model for coding tasks",
            "  2. gpt-5.4              Previous frontier model",
            "Press enter to confirm or esc to go back",
        ])))
        .expect("detection");

        assert_eq!(result.state, State::SlashMenu);
    }

    #[test]
    fn codex_permission_picker_tracks_selected_and_visible_modes() {
        let result = recognize_codex(&lines(&[
            "Update Model Permissions",
            "",
            "› 1. Default      Codex can read and edit files in the current workspace, and run commands. Approval is required to",
            "                  access the internet or edit other files.",
            "  2. Auto-review  Same workspace-write permissions as Default, but eligible `on-request` approvals are routed through",
            "                  the auto-reviewer subagent.",
            "  3. Full Access  Codex can edit files outside this workspace and access the internet without asking for approval.",
            "                  Exercise caution when using.",
            "",
            "  Press enter to confirm or esc to go back",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "codex:permission_picker");
        assert_eq!(result.blocked_kind.as_deref(), Some("permission_picker"));
        assert_eq!(result.phase.as_deref(), Some("permission_mode"));

        let signals = result.screen_signals.expect("codex signals");
        assert!(signals.permission_picker_visible);
        assert_eq!(
            signals.visible_permission_modes,
            vec![
                "Default".to_string(),
                "Auto-review".to_string(),
                "Full Access".to_string()
            ]
        );
        assert_eq!(signals.selected_permission_mode.as_deref(), Some("Default"));
        assert!(signals.last_user_message.is_none());
    }

    #[test]
    fn codex_permission_picker_maps_to_slash_menu_not_confirmation() {
        let result = snapshot_to_detection(recognize_codex(&lines(&[
            "Update Model Permissions",
            "› 1. Default      Codex can read and edit files in the current workspace, and run commands.",
            "  2. Auto-review  Same workspace-write permissions as Default",
            "  3. Full Access  Codex can edit files outside this workspace",
            "Press enter to confirm or esc to go back",
        ])))
        .expect("detection");

        assert_eq!(result.state, State::SlashMenu);
    }

    #[test]
    fn codex_workspace_trust_prompt_is_blocked() {
        let result = recognize_codex(&lines(&[
            "› You are in /Users/jinchen",
            "",
            "  Do you trust the contents of this directory? Working with untrusted contents",
            "  comes with higher risk of prompt injection. Trusting the directory allows",
            "  project-local config, hooks, and exec policies to load.",
            "",
            "› 1. Yes, continue",
            "  2. No, quit",
            "",
            "  Press enter to continue",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "codex:workspace_trust_prompt");
        assert_eq!(result.blocked_kind.as_deref(), Some("workspace_trust"));
        assert_eq!(result.phase.as_deref(), Some("startup_trust"));
    }

    #[test]
    fn codex_mcp_inventory_extracts_server_statuses() {
        let result = recognize_codex(&lines(&[
            "⚠ MCP startup incomplete (failed: missiond_broken)",
            "",
            "🔌 MCP Tools",
            "",
            "  • missiond",
            "    • Auth: Unsupported",
            "    • Tools: mission_board_query, mission_board_create, mission_pty_status",
            "",
            "  • missiond_broken",
            "    • Auth: Unsupported",
            "    • Tools: (none)",
            "",
            "› Use /skills to list available skills",
            "",
            "  gpt-5.5 xhigh · ~/Projects/missiond",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Idle);
        assert_eq!(result.reason, "codex:mcp_startup_incomplete");
        assert_eq!(result.phase.as_deref(), Some("mcp_status"));

        let mcp = result.screen_mcp.expect("mcp screen");
        assert_eq!(mcp.status, "degraded");
        assert_eq!(mcp.failed_servers, vec!["missiond_broken".to_string()]);
        assert!(mcp.startup_incomplete);
        assert_eq!(mcp.servers.len(), 2);
        assert_eq!(mcp.servers[0].name, "missiond");
        assert_eq!(mcp.servers[0].status, "connected");
        assert_eq!(mcp.servers[1].name, "missiond_broken");
        assert_eq!(mcp.servers[1].status, "failed");
    }

    #[test]
    fn codex_mcp_startup_running_is_running() {
        let result = recognize_codex(&lines(&[
            "• Starting MCP servers (4/6): codex_apps, mac-auto-bridge (1s • esc to interrupt)",
            "",
            "› Find and fix a bug in @filename",
            "",
            "  gpt-5.5 xhigh · ~/Projects/missiond",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.reason, "codex:mcp_startup_running");
        assert_eq!(result.phase.as_deref(), Some("mcp_startup"));
        assert!(result
            .screen_mcp
            .as_ref()
            .is_some_and(|mcp| mcp.startup_running));
    }

    #[test]
    fn codex_styled_placeholder_does_not_depend_on_fixed_text() {
        let result = recognize_styled_screen(
            CliEngine::Codex,
            &styled_screen(vec![
                styled_line(vec![styled_span(
                    "│ >_ OpenAI Codex (v0.135.0-alpha.1) │",
                    false,
                    false,
                )]),
                styled_line(vec![styled_span(
                    "│ model:     gpt-5.5 xhigh   /model to change │",
                    false,
                    false,
                )]),
                styled_line(vec![styled_span(
                    "│ directory: ~/Projects/missiond │",
                    false,
                    false,
                )]),
                styled_line(vec![
                    styled_span("›", false, true),
                    styled_span(" ", false, false),
                    styled_span("Improve documentation in @filename", true, false),
                ]),
                styled_line(vec![styled_span(
                    "  gpt-5.5 xhigh · ~/Projects/missiond",
                    false,
                    false,
                )]),
            ]),
            SessionState::Idle,
        );
        assert_eq!(result.state, PtyCanonicalState::Idle);
        let signals = result.screen_signals.expect("codex signals");
        assert!(signals.placeholder_visible);
        assert_eq!(
            signals.placeholder_text.as_deref(),
            Some("Improve documentation in @filename")
        );
        assert_eq!(signals.last_user_message, None);
    }

    #[test]
    fn codex_styled_user_text_wins_over_placeholder_content_match() {
        let result = recognize_styled_screen(
            CliEngine::Codex,
            &styled_screen(vec![
                styled_line(vec![
                    styled_span("›", false, true),
                    styled_span(" ", false, false),
                    styled_span("Find and fix a bug in @filename", false, false),
                ]),
                styled_line(vec![styled_span(
                    "  gpt-5.5 xhigh · ~/Projects/missiond",
                    false,
                    false,
                )]),
            ]),
            SessionState::Idle,
        );
        assert_eq!(result.state, PtyCanonicalState::Idle);
        let signals = result.screen_signals.expect("codex signals");
        assert!(!signals.placeholder_visible);
        assert_eq!(
            signals.last_user_message.as_deref(),
            Some("Find and fix a bug in @filename")
        );
    }

    #[test]
    fn codex_user_and_assistant_messages_are_screen_signals() {
        let result = recognize_codex(&lines(&[
            "› hi",
            "",
            "• Hi. What are we working on in missiond today?",
            "",
            "› Find and fix a bug in @filename",
            "",
            "  gpt-5.5 xhigh · ~/Projects/missiond",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
        let signals = result.screen_signals.expect("codex signals");
        assert_eq!(signals.last_user_message.as_deref(), Some("hi"));
        assert_eq!(
            signals.last_assistant_message.as_deref(),
            Some("Hi. What are we working on in missiond today?")
        );
        assert!(signals.placeholder_visible);
    }

    #[test]
    fn codex_shell_tool_call_is_structured_signal() {
        let result = recognize_codex(&lines(&[
            "› Run pwd using shell.",
            "",
            "• Ran pwd",
            "  └ /Users/jinchen/Projects/missiond",
            "  … +12 lines (ctrl + t to view transcript)",
            "",
            "────────────────────────────────────────────────",
            "",
            "• /Users/jinchen/Projects/missiond",
            "",
            "› Find and fix a bug in @filename",
            "  gpt-5.5 xhigh · ~/Projects/missiond",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
        let signals = result.screen_signals.expect("codex signals");
        assert_eq!(signals.last_tool_kind.as_deref(), Some("shell"));
        assert_eq!(signals.last_tool_label.as_deref(), Some("pwd"));
        assert_eq!(
            signals.last_assistant_message.as_deref(),
            Some("/Users/jinchen/Projects/missiond")
        );
        assert!(signals.folded_tool_output);
        assert_eq!(signals.separator_count, 1);
    }

    #[test]
    fn codex_explore_search_tool_call_is_structured_signal() {
        let result = recognize_codex(&lines(&[
            "› Search for mission_pty_screen.",
            "",
            "• I’ll search the workspace for mission_pty_screen and report where it appears.",
            "",
            "• Explored",
            "  └ Search mission_pty_screen",
            "",
            "────────────────────────────────────────────────",
            "",
            "• Found mission_pty_screen in these places:",
            "",
            "› Find and fix a bug in @filename",
            "  gpt-5.5 xhigh · ~/Projects/missiond",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
        let signals = result.screen_signals.expect("codex signals");
        assert_eq!(signals.last_tool_kind.as_deref(), Some("explore"));
        assert_eq!(
            signals.last_tool_label.as_deref(),
            Some("Search mission_pty_screen")
        );
        assert_eq!(
            signals.last_assistant_message.as_deref(),
            Some("Found mission_pty_screen in these places:")
        );
    }

    #[test]
    fn codex_web_search_running_uses_current_status_line_only() {
        let result = recognize_codex(&lines(&[
            "› Search the internet for today's weather in Shanghai.",
            "",
            "◦ Searching the web",
            "",
            "› Find and fix a bug in @filename",
            "  gpt-5.5 xhigh · ~/Projects/missiond",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.phase.as_deref(), Some("web_search"));
        assert_eq!(result.active_tool.as_deref(), Some("web_search"));
    }

    #[test]
    fn codex_web_search_history_does_not_keep_screen_running() {
        let result = recognize_codex(&lines(&[
            "› Search the internet for today's weather in Shanghai.",
            "",
            "◦ Searching the web",
            "",
            "• Searched https://www.qweather.com/en/weather/shanghai-101020100.html",
            "",
            "────────────────────────────────────────────────",
            "",
            "• Shanghai weather today, June 1, 2026: cloudy.",
            "",
            "› Find and fix a bug in @filename",
            "  gpt-5.5 xhigh · ~/Projects/missiond",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
        let signals = result.screen_signals.expect("codex signals");
        assert_eq!(signals.last_tool_kind.as_deref(), Some("web_search"));
        assert_eq!(
            signals.last_tool_label.as_deref(),
            Some("https://www.qweather.com/en/weather/shanghai-101020100.html")
        );
        assert_eq!(
            signals.last_assistant_message.as_deref(),
            Some("Shanghai weather today, June 1, 2026: cloudy.")
        );
    }

    #[test]
    fn codex_approval_overlay_is_blocked() {
        let result = recognize_codex(&lines(&[
            "Reviewing approval request",
            "Command requires approval",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.blocked_kind.as_deref(), Some("approval"));
    }

    #[test]
    fn codex_mcp_approval_menu_stays_blocked_during_thinking() {
        let result = recognize_screen(
            CliEngine::Codex,
            &lines(&[
                "Field 1/1",
                "Allow the missiond MCP server to run tool \"mission_execution\"?",
                "› 1. Allow",
                "  2. Allow for this session",
                "  3. Always allow",
                "  4. Cancel",
                "enter to submit | esc to cancel",
            ]),
            SessionState::Thinking,
        );
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.source, "tui_source_signature");
        assert_eq!(result.blocked_kind.as_deref(), Some("approval"));
    }

    #[test]
    fn gemini_loading_indicator_is_running() {
        let result = recognize_gemini(&lines(&["⠙ Thinking... (esc to cancel, 5s)", "│ >"]));
        assert_eq!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.phase.as_deref(), Some("thinking"));
    }

    #[test]
    fn gemini_waiting_for_confirmation_is_blocked() {
        let result = recognize_gemini(&lines(&[
            "CoreToolCallStatus.AwaitingApproval",
            "→ Confirming",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
    }

    #[test]
    fn agy_idle_screen_is_idle() {
        let result = recognize_agy(&lines(&[
            "Antigravity 1.107.0",
            "Welcome back Ricky!",
            "> Type your message",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
        assert_eq!(result.reason, "agy:composer_idle");
    }

    #[test]
    fn agy_idle_screen_identity_extracts_logged_in_state() {
        let result = recognize_agy(&lines(&[
            "Antigravity CLI 1.0.3",
            "jjrrqqq@gmail.com (Google AI Ultra)",
            "Gemini 3.5 Flash (Medium)",
            "~/Projects/missiond",
            "Welcome back",
            "> Type your message",
        ]));
        let identity = result.screen_identity.expect("screen identity");
        assert_eq!(identity.cli_version.as_deref(), Some("1.0.3"));
        assert_eq!(identity.account.as_deref(), Some("jjrrqqq@gmail.com"));
        assert_eq!(identity.plan.as_deref(), Some("Google AI Ultra"));
        assert_eq!(
            identity.current_model.as_deref(),
            Some("Gemini 3.5 Flash (Medium)")
        );
        assert_eq!(identity.cwd.as_deref(), Some("~/Projects/missiond"));
        assert_eq!(identity.selected_model, None);
    }

    #[test]
    fn agy_startup_signing_in_is_running() {
        let result = recognize_agy(&lines(&[
            "Welcome to the Antigravity CLI. You are currently not signed in.",
            "⣷  Signing in...",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.reason, "agy:startup_signing_in");
        assert_eq!(result.phase.as_deref(), Some("auth_signing_in"));
    }

    #[test]
    fn agy_oauth_authorization_prompt_is_auth_code_blocked() {
        let result = recognize_screen(
            CliEngine::Agy,
            &lines(&[
                "     ▄▀▀▄",
                "",
                " Open this link in the browser (be sure to copy-paste the whole URL):",
                " ─────────────────────────────────────────────────────────────────",
                " https://accounts.google.com/o/oauth2/auth?access_type=offline&client_id=redacted",
                " .apps.googleusercontent.com&code_challenge=redacted&state=redacted",
                " ─────────────────────────────────────────────────────────────────",
                "",
                " If you aren't automatically redirected, paste the authorization code below:",
                "",
                " authorization code...",
                "",
                "  shift+up/down Navigate",
            ]),
            SessionState::Confirming,
        );

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:oauth_authorization_prompt");
        assert_eq!(result.blocked_kind.as_deref(), Some("auth_code_required"));
        assert_eq!(result.phase.as_deref(), Some("auth_oauth_code"));
        assert_eq!(result.source, "tui_source_signature");
    }

    #[test]
    fn agy_login_method_prompt_is_auth_missing_blocked() {
        let result = recognize_agy(&lines(&[
            "     ▄▀▀▄",
            "    ▀▀▀▀▀▀",
            "   ▀▀▀▀▀▀▀▀",
            "  ▄▀▀    ▀▀▄",
            " ▄▀▀      ▀▀▄",
            "",
            " Welcome to the Antigravity CLI. You are currently not signed in.",
            "",
            " Select login method:",
            " > 1. Google OAuth",
            "   2. Use a Google Cloud project",
            "",
            " [Use arrow keys to navigate, Enter to select]",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:login_method_prompt");
        assert_eq!(result.blocked_kind.as_deref(), Some("auth_missing"));
        assert_eq!(result.phase.as_deref(), Some("auth_login_method"));
        assert_eq!(result.source, "tui_source_signature");
    }

    #[test]
    fn agy_login_method_prompt_overrides_idle_session_state() {
        let result = recognize_screen(
            CliEngine::Agy,
            &lines(&[
                " Welcome to the Antigravity CLI. You are currently not signed in.",
                " Select login method:",
                " > 1. Google OAuth",
                "   2. Use a Google Cloud project",
                " [Use arrow keys to navigate, Enter to select]",
            ]),
            SessionState::Idle,
        );

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:login_method_prompt");
        assert_eq!(result.blocked_kind.as_deref(), Some("auth_missing"));
    }

    #[test]
    fn agy_login_method_ctrl_d_confirm_overrides_auth_prompt() {
        let result = recognize_agy(&lines(&[
            " Welcome to the Antigravity CLI. You are currently not signed in.",
            "",
            " Select login method:",
            " > 1. Google OAuth",
            "   2. Use a Google Cloud project",
            "",
            " [Use arrow keys to navigate, Enter to select]",
            "press ctrl+d again to exit",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:exit_confirm_pending");
        assert_eq!(result.blocked_kind.as_deref(), Some("exit_confirmation"));
        assert_eq!(result.phase.as_deref(), Some("exit_confirm"));
    }

    #[test]
    fn agy_workspace_trust_prompt_is_blocked() {
        let result = recognize_agy(&lines(&[
            "Accessing workspace:",
            "/Users/jinchen",
            "Do you trust the contents of this project?",
            "Antigravity CLI requires permission to read, edit, and execute files here.",
            "> Yes, I trust this folder",
            "  No, exit",
            "↑/↓ Navigate · enter Confirm",
            "Claude Opus 4.6 (Thinking)",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:workspace_trust_prompt");
        assert_eq!(result.blocked_kind.as_deref(), Some("workspace_trust"));
        assert_eq!(result.phase.as_deref(), Some("startup_trust"));
    }

    #[test]
    fn agy_model_picker_tracks_current_and_selected_model() {
        let result = recognize_agy(&lines(&[
            "Switch Model",
            "Gemini 3.5 Flash (Medium) (current)",
            "> Gemini 3.5 Flash (High)",
            "Gemini 3.5 Flash (Low)",
            "Gemini 3.1 Pro (Low)",
            "Gemini 3.1 Pro (High)",
            "Claude Sonnet 4.6 (Thinking)",
            "Claude Opus 4.6 (Thinking)",
            "GPT-OSS 120B (Medium)",
            "Keyboard:",
            "Up/Down Navigate enter Select esc Go Back",
            "esc to cancel                                                                                    Gemini 3.5 Flash (Medium)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:model_picker");
        assert_eq!(result.blocked_kind.as_deref(), Some("model_picker"));

        let identity = result.screen_identity.expect("screen identity");
        assert_eq!(
            identity.current_model.as_deref(),
            Some("Gemini 3.5 Flash (Medium)")
        );
        assert_eq!(
            identity.selected_model.as_deref(),
            Some("Gemini 3.5 Flash (High)")
        );
    }

    #[test]
    fn agy_model_picker_maps_to_slash_menu_not_confirmation() {
        let result = snapshot_to_detection(recognize_agy(&lines(&[
            "Switch Model",
            "Gemini 3.5 Flash (Medium) (current)",
            "> Claude Opus 4.6 (Thinking)",
            "Keyboard:",
            "Up/Down Navigate enter Select esc Go Back",
            "esc to cancel                                                                                    Gemini 3.5 Flash (Medium)",
        ])))
        .expect("detection");

        assert_eq!(result.state, State::SlashMenu);
    }

    #[test]
    fn agy_usage_meter_extracts_visible_model_quotas() {
        let result = recognize_agy(&lines(&[
            "└ Model Quota",
            "Gemini 3.5 Flash (Medium)",
            "███████████ ███████████ ███████████ ███████████ ███████████",
            "100%",
            "Quota available",
            "Gemini 3.5 Flash (High)",
            "███████████ ███████████ ███████████ ███████████ ███████████",
            "100%",
            "Quota available",
            "Claude Sonnet 4.6 (Thinking)",
            "███████████ ███████████ ███████████ ███████████ ███████████",
            "100%",
            "Quota available",
            "(1–28 of 33 lines)",
            "↑/↓ Scroll · pgup/pgdown Page · ctrl+end Bottom · ctrl+home Top · esc Close",
            "esc to cancel                                                                                    Gemini 3.5 Flash (High)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Complete);
        assert_eq!(result.reason, "agy:usage_meter");
        assert_eq!(result.phase.as_deref(), Some("usage_meter"));
        let identity = result.screen_identity.expect("screen identity");
        assert_eq!(
            identity.current_model.as_deref(),
            Some("Gemini 3.5 Flash (High)")
        );

        let usage = result.screen_usage.expect("screen usage");
        assert_eq!(usage.title, "Model Quota");
        assert_eq!(
            usage.visible_range,
            Some(ProviderVisibleRange {
                start: 1,
                end: 28,
                total: 33
            })
        );
        assert_eq!(usage.model_quotas.len(), 3);
        assert_eq!(usage.model_quotas[0].model, "Gemini 3.5 Flash (Medium)");
        assert_eq!(usage.model_quotas[0].percent, Some(100));
        assert_eq!(
            usage.model_quotas[0].status.as_deref(),
            Some("Quota available")
        );
        assert_eq!(usage.model_quotas[2].model, "Claude Sonnet 4.6 (Thinking)");
    }

    #[test]
    fn agy_usage_meter_with_blank_rows_overrides_confirming_session_state() {
        let result = recognize_screen(
            CliEngine::Agy,
            &lines(&[
                "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
                ">",
                "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
                "└ Model Quota",
                "",
                "  Gemini 3.5 Flash (Medium)",
                "  ███████████ ███████████ ███████████ ███████████ ███████████ 100%",
                "  Quota available",
                "",
                "  Gemini 3.5 Flash (High)",
                "  ███████████ ███████████ ███████████ ███████████ ███████████ 100%",
                "  Quota available",
                "",
                "  Gemini 3.5 Flash (Low)",
                "  ███████████ ███████████ ███████████ ███████████ ███████████ 100%",
                "  Quota available",
                "",
                "  (1–20 of 33 lines)",
                "",
                "  ↑/↓ Scroll · pgup/pgdown Page · ctrl+end Bottom · ctrl+home Top · esc Close",
                "esc to cancel                                                                                      GPT-OSS 120B (Medium)",
            ]),
            SessionState::Confirming,
        );

        assert_eq!(result.state, PtyCanonicalState::Complete);
        assert_eq!(result.reason, "agy:usage_meter");
        let usage = result.screen_usage.expect("screen usage");
        assert_eq!(usage.model_quotas.len(), 3);
        assert_eq!(usage.model_quotas[0].model, "Gemini 3.5 Flash (Medium)");
        assert_eq!(usage.model_quotas[0].percent, Some(100));
        assert_eq!(
            usage.model_quotas[0].status.as_deref(),
            Some("Quota available")
        );
    }

    #[test]
    fn agy_help_output_is_completed_diagnostic_evidence() {
        let result = recognize_screen(
            CliEngine::Agy,
            &lines(&[
                "(missiond-teach) missiond % /Users/jinchen/.local/bin/agy --help",
                "Usage of /Users/jinchen/.local/bin/agy:",
                "  --add-dir                       Add a directory to the workspace (repeatable) (default [])",
                "  -c                              Short alias for --continue",
                "  --continue                      Continue the most recent conversation",
                "  --conversation                  Resume a previous conversation by ID",
                "  --dangerously-skip-permissions  Auto-approve all tool permission requests without prompting",
                "  -i                              Short alias for --prompt-interactive",
                "  --log-file                      Override CLI log file path",
                "  -p                              Short alias for --print",
                "  --print                         Run a single prompt non-interactively and print the response",
                "  --print-timeout                 Timeout for print mode wait (default 5m0s)",
                "  --prompt                        Alias for --print",
                "  --prompt-interactive            Run an initial prompt interactively and continue the session",
                "  --sandbox                       Run in a sandbox with terminal restrictions enabled",
                "",
                "Available subcommands:",
                "  changelog       Show changelog and release notes",
                "  help            Show help for subcommands",
                "  install         Configure environment paths and shell settings",
                "  plugin          Manage plugins (install, uninstall, list, enable, disable)",
                "  plugins         Alias for plugin",
                "  update          Update CLI",
                "(missiond-teach) missiond %",
            ]),
            SessionState::Starting,
        );

        assert_eq!(result.state, PtyCanonicalState::Complete);
        assert_eq!(result.reason, "agy:cli_help");
        assert_eq!(result.phase.as_deref(), Some("cli_help"));
        assert_eq!(result.source, "cli_help_signature");
    }

    #[test]
    fn agy_generating_spinner_is_running_even_when_prompt_visible() {
        let result = recognize_agy(&lines(&[
            "请写 20 行很短的中文句子，主题是 PTY 队列状态识别，每行编号。",
            "⣽ Generating...",
            "⣷ Working...",
            ">",
            "esc to cancel                                                                                    Gemini 3.5 Flash (High)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.reason, "agy:active_status");
        assert_eq!(result.phase.as_deref(), Some("thinking"));
    }

    #[test]
    fn agy_stale_spinner_with_ready_footer_is_idle() {
        let result = recognize_agy(&lines(&[
            "请写 20 行很短的中文句子，主题是 PTY 队列状态识别，每行编号。",
            "⣽ Generating...",
            "⣷ Working...",
            "1. PTY 队列需要观察状态。",
            "2. 工位一次只处理一个请求。",
            "────────────────────────────────────────",
            ">",
            "────────────────────────────────────────",
            "? for shortcuts                                                                                  Gemini 3.5 Flash (High)",
        ]));
        assert_ne!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.state, PtyCanonicalState::Idle);
        assert_eq!(result.reason, "agy:composer_idle");
    }

    #[test]
    fn agy_interrupted_prompt_is_idle_ready_for_retry() {
        let result = recognize_agy(&lines(&[
            "20. 队列清理完成无遗留。",
            "⎿ Interrupted · What should Antigravity CLI do instead?",
            "────────────────────────────────────────",
            ">",
            "────────────────────────────────────────",
            "? for shortcuts                                                                                  Gemini 3.5 Flash (High)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
        assert_eq!(result.reason, "agy:interrupted_ready_for_retry");
        assert_eq!(result.phase.as_deref(), Some("interrupted"));
    }

    #[test]
    fn agy_slash_command_menu_is_not_ready_for_prompt_input() {
        let result = recognize_agy(&lines(&[
            "────────────────────────────────────────",
            "> /c",
            "────────────────────────────────────────",
            "/changelog        Show release notes and changes",
            "> /clear          Clear conversation and start a new one",
            "/config           Open settings panel",
            "/context          Visualize current context usage",
            "/copy             Copy the last planner response to the clipboard",
            "↓ 2 more",
            "↑/↓ Navigate · enter Select · tab Complete",
            "esc to cancel                                                                                    Gemini 3.5 Flash (High)",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:slash_command_menu");
        assert_eq!(result.blocked_kind.as_deref(), Some("slash_command_menu"));
        assert_eq!(result.phase.as_deref(), Some("command_menu"));
    }

    #[test]
    fn agy_completed_slash_command_input_is_pending_not_executed() {
        let result = recognize_agy(&lines(&[
            "────────────────────────────────────────",
            "> /clear",
            "────────────────────────────────────────",
            "? for shortcuts                                                                                  Gemini 3.5 Flash (High)",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:slash_command_pending");
        assert_eq!(result.blocked_kind.as_deref(), Some("slash_command_input"));
        assert_eq!(result.phase.as_deref(), Some("command_input"));
    }

    #[test]
    fn agy_completed_slash_command_history_with_empty_prompt_is_ready() {
        let result = recognize_agy(&lines(&[
            "      ▄▀▀▄        Antigravity CLI 1.0.3",
            "     ▀▀▀▀▀▀       jjrrqqq@gmail.com (Google AI Ultra)",
            "    ▀▀▀▀▀▀▀▀      Gemini 3.1 Pro (High)",
            "   ▄▀▀    ▀▀▄     ~/Projects/missiond",
            "> /model",
            "  ⎿  Model set to Gemini 3.1 Pro (High)",
            "────────────────────────────────────────",
            ">",
            "────────────────────────────────────────",
            "? for shortcuts                                                                                    Gemini 3.1 Pro (High)",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Idle);
        assert_eq!(result.reason, "agy:composer_idle");
        assert_eq!(result.blocked_kind, None);
    }

    #[test]
    fn agy_idle_screen_with_absolute_cwd_is_not_slash_command_pending() {
        let result = recognize_agy(&lines(&[
            "Antigravity CLI 1.0.3",
            "jjrrqqq@gmail.com (Google AI Ultra)",
            "Gemini 3.5 Flash (Medium)",
            "/Users/jinchen/.missiond/runtime/missiond/provider-box/agy-text-only/slot-agy-gemini-35-flash-high-a/workspace",
            "────────────────────────────────────────",
            ">",
            "────────────────────────────────────────",
            "? for shortcuts                                                                                Gemini 3.5 Flash (Medium)",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Idle);
        assert_eq!(result.reason, "agy:composer_idle");
        assert_eq!(result.blocked_kind, None);
    }

    #[test]
    fn agy_wrapped_url_path_is_not_misread_as_cwd() {
        let result = recognize_agy(&lines(&[
            "Antigravity CLI 1.0.3",
            "jjrrqqq@gmail.com (Google AI Ultra)",
            "Claude Opus 4.6 Thinking",
            "https://www.m1-project.com/blog/research-source-that-wrapped",
            "/www.m1-project.com/blog/research-source-that-wrapped",
            "────────────────────────────────────────",
            ">",
            "────────────────────────────────────────",
            "? for shortcuts                                                                                  Claude Opus 4.6 Thinking",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Idle);
        assert_eq!(
            result
                .screen_identity
                .as_ref()
                .and_then(|identity| identity.cwd.as_deref()),
            None
        );
    }

    #[test]
    fn agy_mcp_servers_page_is_blocked_menu_not_composer_idle() {
        let result = recognize_agy(&lines(&[
            "      ▄▀▀▄        Antigravity CLI 1.0.3",
            "     ▀▀▀▀▀▀       jjrrqqq@gmail.com (Google AI Ultra)",
            "    ▀▀▀▀▀▀▀▀      Gemini 3.5 Flash (Medium)",
            "   ▄▀▀    ▀▀▄     ~/Projects/missiond",
            "────────────────────────────────────────",
            "MCP Servers",
            "Plugins (~/.gemini/antigravity-cli/plugins)",
            ">  ✓ missiond  Tools: mission_board_query, mission_board_create, mission_board_update, mission_board_delete,",
            "               mission_board_claim, +93 more",
            "   ✗ missiond-reconnect-teach  error: missiond reconnect teaching failure : calling \"initialize\": EOF",
            "Keyboard: ↑/↓ Navigate  enter Actions  esc to cancel                                      Gemini 3.5 Flash (Medium)",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:mcp_servers");
        assert_eq!(result.blocked_kind.as_deref(), Some("mcp_servers"));
        assert_eq!(result.phase.as_deref(), Some("mcp_status"));
    }

    #[test]
    fn agy_ctrl_d_first_press_waits_for_second_press() {
        let result = recognize_agy(&lines(&[
            "Antigravity CLI 1.0.3",
            "jjrrqqq@gmail.com (Google AI Ultra)",
            "Gemini 3.5 Flash (High)",
            "~/Projects/missiond",
            "────────────────────────────────────────",
            ">",
            "────────────────────────────────────────",
            "press ctrl+d again to exit                                                                        Gemini 3.5 Flash (High)",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:exit_confirm_pending");
        assert_eq!(result.blocked_kind.as_deref(), Some("exit_confirmation"));
    }

    #[test]
    fn agy_ctrl_d_second_press_returns_to_shell_prompt() {
        let result = recognize_agy(&lines(&[
            "Resume with:",
            "  agy --conversation=917a5c67-e5b7-467a-8cfa-0d142faa474a",
            "  agy -c",
            "Antigravity CLI 1.0.3",
            "jjrrqqq@gmail.com (Google AI Ultra)",
            "Gemini 3.5 Flash (High)",
            "~/Projects/missiond",
            "Resume: agy --conversation=917a5c67-e5b7-467a-8cfa-0d142faa474a (or -c)",
            "(base) jinchen@Mac missiond %",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Complete);
        assert_eq!(result.reason, "agy:shell_prompt_after_exit");
        assert_eq!(result.phase.as_deref(), Some("exited"));
    }

    #[test]
    fn agy_feedback_prompt_after_answer_is_complete() {
        let result = recognize_agy(&lines(&[
            "## Findings",
            "- Agy read-only lane completed the requested review.",
            "",
            "How's the CLI experience so far? Help us improve:",
            "[1] Good  [2] Fine  [3] Bad  [0] Skip",
            "esc to cancel                                                                                    Gemini 3.5 Flash (High)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Complete);
        assert_eq!(result.reason, "agy:feedback_prompt_after_complete");
    }

    #[test]
    fn agy_auth_or_quota_error_is_blocked() {
        let result = recognize_screen(
            CliEngine::Agy,
            &lines(&["quota exceeded; please check billing to continue"]),
            SessionState::Error,
        );
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.source, "screen_final");
        assert_eq!(result.blocked_kind.as_deref(), Some("usage_limit"));
    }

    #[test]
    fn agy_file_access_prompt_is_blocked_confirmation() {
        let result = recognize_screen(
            CliEngine::Agy,
            &lines(&[
                "File access",
                "Read: /Users/rickyhq/.missiond/runtime/missiond/context-gather/abc.json",
                "Reason: outside workspace",
                "Allow access to this file?",
                "> 1. Yes, allow access",
                "  2. Yes, and always allow non-workspace access",
                "  3. No, deny access",
            ]),
            SessionState::Thinking,
        );
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "agy:file_access_approval");
        assert_eq!(result.source, "tui_source_signature");
        assert_eq!(result.blocked_kind.as_deref(), Some("approval"));
    }

    #[test]
    fn claude_spinner_is_running() {
        let result = recognize_claude_code(&lines(&[
            "* Combobulating... (2m 42s · ↓ 6.3k tokens · almost done thinking with high effort)",
            "›",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.elapsed_secs, Some(162));
    }

    #[test]
    fn claude_code_spinner_verb_with_active_footer_is_running() {
        // ClaudeCode's spinner text comes from src/constants/spinnerVerbs.ts
        // and can be any randomized "<verb>…" phrase, e.g. Whirlpooling.
        // The current-turn evidence is the user prompt plus active footer,
        // not the specific verb.
        let result = recognize_claude_code(&lines(&[
            "❯ /logout",
            "· Whirlpooling…",
            "────────────────────────────────────────",
            "❯",
            "────────────────────────────────────────",
            "⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.reason, "claude_code:logout_running");
        assert_eq!(result.phase.as_deref(), Some("logout"));
    }

    #[test]
    fn claude_completion_and_prompt_is_complete() {
        let result = recognize_claude_code(&lines(&[
            "* Worked for 10s",
            "› auto mode on (shift+tab to cycle)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Complete);
    }

    #[test]
    fn claude_code_idle_screen_extracts_identity() {
        let result = recognize_claude_code(&lines(&[
            " ▐▛███▜▌   Claude Code v2.1.159",
            "▝▜█████▛▘  Opus 4.8 (1M context) with xhigh effort · Claude Max",
            "  ▘▘ ▝▝    ~/Projects/missiond",
            "",
            "────────────────────────────────────────────────────────────────",
            "❯",
            "────────────────────────────────────────────────────────────────",
            "⏵⏵ auto mode on (shift+tab to cycle) · ← for agents",
        ]));

        assert_eq!(result.state, PtyCanonicalState::Idle);
        let identity = result.screen_identity.expect("claude code identity");
        assert_eq!(identity.cli_version.as_deref(), Some("2.1.159"));
        assert_eq!(identity.current_model.as_deref(), Some("Opus 4.8"));
        assert_eq!(identity.reasoning_effort.as_deref(), Some("xhigh"));
        assert_eq!(identity.plan.as_deref(), Some("Claude Max"));
        assert_eq!(identity.cwd.as_deref(), Some("~/Projects/missiond"));
        assert_eq!(identity.permission_mode.as_deref(), Some("auto"));
    }

    #[test]
    fn claude_code_permission_footer_modes_are_structured_identity() {
        for (footer, expected) in [
            ("? for shortcuts · ← for agents", "default"),
            (
                "⏵⏵ accept edits on (shift+tab to cycle) · ← for agents",
                "accept_edits",
            ),
            ("⏸ plan mode on (shift+tab to cycle) · ← for agents", "plan"),
            (
                "⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents",
                "bypass_permissions",
            ),
        ] {
            let result = recognize_claude_code(&lines(&[
                " ▐▛███▜▌   Claude Code v2.1.159",
                "▝▜█████▛▘  Opus 4.8 (1M context) with xhigh effort · Claude Max",
                "  ▘▘ ▝▝    ~/Projects/missiond",
                "❯",
                footer,
            ]));
            let identity = result.screen_identity.expect("claude code identity");
            assert_eq!(
                identity.permission_mode.as_deref(),
                Some(expected),
                "footer: {footer}"
            );
        }
    }

    #[test]
    fn claude_code_slash_menu_preserves_identity_when_session_state_fallbacks() {
        let result = recognize_screen(
            CliEngine::ClaudeCode,
            &lines(&[
                " ▐▛███▜▌   Claude Code v2.1.159",
                "▝▜█████▛▘  Opus 4.8 (1M context) with xhigh effort · Claude Max",
                "  ▘▘ ▝▝    ~/Projects/missiond",
                "",
                "❯ /l",
                "/loop                                  Run a prompt or slash command on a recurring interval",
                "/login                                 Sign in with your Anthropic account",
                "/logout                                Sign out from your Anthropic account",
            ]),
            SessionState::Idle,
        );

        assert_eq!(result.state, PtyCanonicalState::Idle);
        let identity = result.screen_identity.expect("claude code identity");
        assert_eq!(identity.cli_version.as_deref(), Some("2.1.159"));
        assert_eq!(identity.current_model.as_deref(), Some("Opus 4.8"));
        assert_eq!(identity.reasoning_effort.as_deref(), Some("xhigh"));
        assert_eq!(identity.plan.as_deref(), Some("Claude Max"));
        assert_eq!(identity.cwd.as_deref(), Some("~/Projects/missiond"));
    }

    #[test]
    fn fusion_active_session_overrides_stale_confirmation_text() {
        // Screen carries leftover confirmation/picker words from a moment ago,
        // but the worker is already actively running per SessionState. Fusion
        // must produce Running grounded in the live spinner, not Blocked.
        let result = recognize_screen(
            CliEngine::ClaudeCode,
            &lines(&[
                "Do you want to proceed with this approval? (y/n)",
                "Select model: Sonnet 4",
                "* Spelunking... (3s · esc to interrupt)",
                "›",
            ]),
            SessionState::ToolRunning,
        );
        assert_eq!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.source, "screen_fused");
        assert_eq!(result.reason, "claude_code:active_spinner");
        assert!(result.blocked_kind.is_none());
    }

    #[test]
    fn fusion_active_session_without_active_screen_falls_back_to_session_state() {
        // Screen has confirmation text but no active spinner / esc-to-interrupt.
        // Session is Thinking, so the stale Blocked must demote to Running
        // sourced from session_state, not stay Blocked.
        let result = recognize_screen(
            CliEngine::ClaudeCode,
            &lines(&["Do you want to proceed?", "permission to read /etc/hosts"]),
            SessionState::Thinking,
        );
        assert_eq!(result.state, PtyCanonicalState::Running);
        assert_eq!(result.source, "session_state");
        assert!(result.reason.starts_with("session_state:"));
        assert!(result.blocked_kind.is_none());
    }

    #[test]
    fn fusion_exited_session_state_overrides_stale_running_screen() {
        // Live Codex startup shape: an auto-update spinner remained on screen
        // after the process exited with "Please restart Codex". The durable
        // session state must win so frontends/watchdogs do not show stale
        // running status.
        let result = recognize_screen(
            CliEngine::Codex,
            &lines(&[
                "Updating Codex via `npm install -g @openai/codex`...",
                "🎉  Update ran successfully! Please restart Codex.",
                "⠴",
            ]),
            SessionState::Exited,
        );
        assert_eq!(result.state, PtyCanonicalState::Complete);
        assert_eq!(result.source, "session_state");
        assert_eq!(result.reason, "session_state:Exited");
    }

    #[test]
    fn claude_code_auth_missing_is_blocked_even_after_exit() {
        let result = recognize_screen(
            CliEngine::ClaudeCode,
            &lines(&[
                "Credentials file not found — Claude Code may require interactive login",
                "Please log in to continue.",
            ]),
            SessionState::Exited,
        );
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.source, "screen_final");
        assert_eq!(result.reason, "provider:auth_missing");
        assert_eq!(result.blocked_kind.as_deref(), Some("auth_missing"));
    }

    #[test]
    fn claude_code_billing_pause_is_blocked_even_after_exit() {
        let result = recognize_screen(
            CliEngine::ClaudeCode,
            &lines(&[
                "Your account has been paused because payment failed.",
                "Update your billing details to continue using Claude Code.",
            ]),
            SessionState::Exited,
        );
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.source, "screen_final");
        assert_eq!(result.reason, "provider:billing_or_account");
        assert_eq!(result.blocked_kind.as_deref(), Some("billing_or_account"));
    }

    #[test]
    fn gemini_quota_error_is_blocked() {
        let result = recognize_screen(
            CliEngine::Gemini,
            &lines(&["TerminalQuotaError: exhausted your daily quota"]),
            SessionState::Error,
        );
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.source, "screen_final");
        assert_eq!(result.blocked_kind.as_deref(), Some("usage_limit"));
    }

    #[test]
    fn fusion_true_confirming_state_keeps_blocked() {
        // Same confirmation text, but SessionState explicitly says Confirming.
        // The screen evidence and the session state agree -- preserve Blocked.
        let result = recognize_screen(
            CliEngine::ClaudeCode,
            &lines(&["Do you want to proceed?", "permission to read /etc/hosts"]),
            SessionState::Confirming,
        );
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.blocked_kind.as_deref(), Some("confirmation"));
        assert_eq!(result.reason, "claude_code:confirmation_or_picker");
    }

    #[test]
    fn fusion_does_not_promote_idle_session_state() {
        // Confirmation text on screen with an Idle SessionState must keep the
        // screen-derived Blocked (no active processing to fuse with).
        let result = recognize_screen(
            CliEngine::ClaudeCode,
            &lines(&["Do you want to proceed?"]),
            SessionState::Idle,
        );
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "claude_code:confirmation_or_picker");
    }

    #[test]
    fn claude_code_completed_screen_with_permission_footer_is_complete() {
        // Live false-positive shape: turn finished, brief still mentions
        // approval generically, footer shows the "bypass permissions on"
        // composer-mode toggle. Recognition must report Complete, not Blocked.
        let result = recognize_claude_code(&lines(&[
            "Brief: route the approval workflow through the new gate",
            "* Worked for 12s",
            "› bypass permissions on (shift+tab to cycle)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Complete);
        assert!(result.blocked_kind.is_none());
    }

    #[test]
    fn claude_code_idle_screen_with_permission_footer_is_idle() {
        // Same false-positive shape minus the completion line: still must
        // not be Blocked just because the scroll buffer carries `approval`
        // and the footer is the bypass-permissions toggle.
        let result = recognize_claude_code(&lines(&[
            "Earlier discussion of the approval workflow stays here.",
            "› bypass permissions on (shift+tab to cycle)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
        assert!(result.blocked_kind.is_none());
    }

    #[test]
    fn claude_code_idle_prompt_overrides_stale_spinner_text() {
        // Live Jarvis failure shape: Claude Code read one skill, returned to
        // the composer prompt, but the scrollback still carried an old
        // "Puzzling" spinner line. The prompt is the current state signal.
        let result = recognize_claude_code(&lines(&[
            "Reading 1 file... (ctrl+o to expand)",
            "⎿  ~/.claude/skills/xiaojin-blog/SKILL.md",
            "✳ Puzzling… (17s · ↓ 706 tokens)",
            "────────────────────────────────────────",
            "❯",
            "────────────────────────────────────────",
            "⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Idle);
        assert_eq!(result.reason, "claude_code:prompt_idle");
    }

    #[test]
    fn claude_code_first_run_theme_prompt_is_startup_blocked() {
        let result = recognize_claude_code(&lines(&[
            "Welcome to Claude Code v2.1.159",
            "Let's get started.",
            "Choose the text style that looks best with your terminal",
            "  1. Auto (match terminal)",
            "❯ 2. Dark mode ✔",
            "  3. Light mode",
            "  4. Dark mode (colorblind-friendly)",
            "",
            "Syntax theme: Monokai Extended (ctrl+t to disable)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "claude_code:first_run_theme_prompt");
        assert_eq!(result.blocked_kind.as_deref(), Some("startup_config"));
        assert_eq!(result.phase.as_deref(), Some("startup_theme"));
        let identity = result.screen_identity.expect("claude code identity");
        assert_eq!(identity.cli_version.as_deref(), Some("2.1.159"));
        let signals = result.screen_signals.expect("startup signals");
        assert!(signals.startup_prompt_visible);
        assert_eq!(signals.startup_prompt_kind.as_deref(), Some("theme_picker"));
        assert_eq!(signals.selected_startup_option_index, Some(2));
        assert_eq!(
            signals.selected_startup_option.as_deref(),
            Some("Dark mode")
        );
        assert!(signals.selected_startup_option_checked);
        assert!(signals
            .visible_startup_options
            .contains(&"Auto (match terminal)".to_string()));
    }

    #[test]
    fn claude_code_login_method_prompt_is_auth_missing_blocked() {
        let result = recognize_claude_code(&lines(&[
            "Welcome to Claude Code v2.1.159",
            "Claude Code can be used with your Claude subscription or billed based on API usage through your Console account.",
            "Select login method:",
            "❯ 1. Claude account with subscription · Pro, Max, Team, or Enterprise",
            "  2. Anthropic Console account · API usage billing",
            "  3. 3rd-party platform · Amazon Bedrock, Microsoft Foundry, or Vertex AI",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "claude_code:login_method_prompt");
        assert_eq!(result.blocked_kind.as_deref(), Some("auth_missing"));
        assert_eq!(result.phase.as_deref(), Some("auth_login_method"));
        let identity = result.screen_identity.expect("claude code identity");
        assert_eq!(identity.cli_version.as_deref(), Some("2.1.159"));
        let signals = result.screen_signals.expect("startup signals");
        assert_eq!(signals.startup_prompt_kind.as_deref(), Some("login_method"));
        assert_eq!(signals.selected_startup_option_index, Some(1));
        assert_eq!(
            signals.selected_startup_option.as_deref(),
            Some("Claude account with subscription · Pro, Max, Team, or Enterprise")
        );
        assert!(!signals.selected_startup_option_checked);
    }

    #[test]
    fn claude_code_oauth_authorization_prompt_is_auth_code_blocked() {
        let result = recognize_claude_code(&lines(&[
            "Welcome to Claude Code v2.1.159",
            "Browser didn't open? Use the url below to sign in (c to copy)",
            "https://claude.com/cai/oauth/authorize?client_id=[REDACTED]&code_challenge=[REDACTED]&state=[REDACTED]",
            "",
            "Paste code here if prompted >",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "claude_code:oauth_authorization_prompt");
        assert_eq!(result.blocked_kind.as_deref(), Some("auth_code_required"));
        assert_eq!(result.phase.as_deref(), Some("auth_oauth"));
        let identity = result.screen_identity.expect("claude code identity");
        assert_eq!(identity.cli_version.as_deref(), Some("2.1.159"));
        let signals = result.screen_signals.expect("startup signals");
        assert!(signals.startup_prompt_visible);
        assert_eq!(
            signals.startup_prompt_kind.as_deref(),
            Some("oauth_authorization")
        );
        assert!(signals.visible_startup_options.is_empty());
    }

    #[test]
    fn claude_code_login_success_continue_extracts_account() {
        let result = recognize_claude_code(&lines(&[
            "Welcome to Claude Code v2.1.159",
            "..........................................................",
            "",
            " Logged in as user@example.com",
            " Login successful. Press Enter to continue…",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "claude_code:login_success_continue");
        assert_eq!(result.blocked_kind.as_deref(), Some("startup_continue"));
        assert_eq!(result.phase.as_deref(), Some("auth_login_success"));
        let identity = result.screen_identity.expect("claude code identity");
        assert_eq!(identity.cli_version.as_deref(), Some("2.1.159"));
        assert_eq!(identity.account.as_deref(), Some("user@example.com"));
        let signals = result.screen_signals.expect("startup signals");
        assert!(signals.startup_prompt_visible);
        assert_eq!(
            signals.startup_prompt_kind.as_deref(),
            Some("login_success_continue")
        );
        assert!(signals.visible_startup_options.is_empty());
    }

    #[test]
    fn claude_code_security_notes_continue_is_startup_blocked() {
        let result = recognize_claude_code(&lines(&[
            "Welcome to Claude Code v2.1.159",
            "",
            " Security notes:",
            "",
            " 1. Claude can make mistakes.",
            "    You're responsible for Claude's actions and should always",
            "    review them, especially when running code.",
            "",
            " 2. Due to prompt injection risks, only use it with code you trust",
            "    Learn more: https://code.claude.com/docs/en/security",
            "",
            " Press Enter to continue…",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "claude_code:security_notes_continue");
        assert_eq!(result.blocked_kind.as_deref(), Some("startup_continue"));
        assert_eq!(result.phase.as_deref(), Some("startup_security_notes"));
        let identity = result.screen_identity.expect("claude code identity");
        assert_eq!(identity.cli_version.as_deref(), Some("2.1.159"));
        let signals = result.screen_signals.expect("startup signals");
        assert!(signals.startup_prompt_visible);
        assert_eq!(
            signals.startup_prompt_kind.as_deref(),
            Some("security_notes_continue")
        );
    }

    #[test]
    fn fusion_idle_session_with_permission_footer_is_not_blocked() {
        // Mirrors the live mission_pty_status snapshot: SessionState=Idle but
        // screen still contains historical `approval` prose plus the
        // `bypass permissions on` footer. The fused recognition MUST NOT be
        // Blocked, because no explicit modal is on screen.
        let result = recognize_screen(
            CliEngine::ClaudeCode,
            &lines(&[
                "Brief: handle approval flow for the migration",
                "* Worked for 7s",
                "› bypass permissions on (shift+tab to cycle)",
            ]),
            SessionState::Idle,
        );
        assert_ne!(result.state, PtyCanonicalState::Blocked);
        assert!(result.blocked_kind.is_none());
        assert_eq!(result.state, PtyCanonicalState::Complete);
    }

    #[test]
    fn claude_code_explicit_confirmation_modal_is_blocked() {
        let result = recognize_claude_code(&lines(&[
            "Do you want to proceed?",
            "❯ 1. Yes",
            "  2. No, and tell Claude what to do differently (esc)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.blocked_kind.as_deref(), Some("confirmation"));
        assert_eq!(result.reason, "claude_code:confirmation_or_picker");
    }

    #[test]
    fn claude_code_model_picker_is_blocked() {
        let result = recognize_claude_code(&lines(&[
            "Select model",
            "Switch between available Claude models",
            "❯ Opus 4.7",
            "  Sonnet 4.6",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.blocked_kind.as_deref(), Some("confirmation"));
    }

    #[test]
    fn claude_code_mcp_list_is_structured_status_page() {
        let result = recognize_claude_code(&lines(&[
            "  Manage MCP servers",
            "  12 servers",
            "  ⚠ 2 setup issues: MCP · /doctor",
            "",
            "    Local MCPs (/Users/jinchen/.claude.json [project: /private/tmp/missiond-search-noise-fix])",
            "  ❯ chrome-devtools · ✔ connected · 29 tools",
            "    missiond-fail-demo · ✘ failed",
            "",
            "    User MCPs (/Users/jinchen/.claude.json)",
            "    context7 · ✔ connected · 2 tools",
            "    claude.ai Gmail · △ needs authentication",
            "",
            "    Built-in MCPs (always available)",
            "    computer-use · ◯ disabled",
            "    missiond · ✔ connected · 100 tools",
            "  ※ Run claude --debug to see error logs",
            "  https://code.claude.com/docs/en/mcp for help",
            " ↑/↓ to navigate · Enter to confirm · Esc to cancel",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "claude_code:mcp_servers");
        assert_eq!(result.blocked_kind.as_deref(), Some("mcp_servers"));
        let mcp = result.screen_mcp.expect("mcp screen");
        assert_eq!(mcp.status, "degraded");
        assert_eq!(mcp.failed_servers, vec!["missiond-fail-demo".to_string()]);
        assert!(!mcp
            .servers
            .iter()
            .any(|server| server.name.to_ascii_lowercase().contains("setup issue")));
        assert!(mcp
            .servers
            .iter()
            .any(|server| server.name == "missiond" && server.connected));
        assert!(mcp.servers.iter().any(|server| {
            server.name == "claude.ai Gmail" && server.status == "needs_authentication"
        }));
    }

    #[test]
    fn claude_code_mcp_detail_is_structured_status_page() {
        let result = recognize_claude_code(&lines(&[
            "  Missiond-fail-demo MCP Server",
            "",
            "  Status:           ✘ failed",
            "  Command:          /bin/sh",
            "  Args:             -c echo missiond-fail-demo-stderr >&2; exit 1",
            "  Config location:  /Users/jinchen/.claude.json [project: /private/tmp/missiond-search-noise-fix]",
            "",
            "  ❯ 1. Reconnect",
            "    2. Disable",
            "",
            "  ↑/↓ to navigate · Enter to select · Esc to back",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
        assert_eq!(result.reason, "claude_code:mcp_servers");
        let mcp = result.screen_mcp.expect("mcp detail");
        assert_eq!(mcp.title, "Missiond-fail-demo MCP Server");
        assert_eq!(mcp.servers[0].name, "Missiond-fail-demo");
        assert_eq!(mcp.servers[0].status, "failed");
        assert_eq!(mcp.failed_servers, vec!["Missiond-fail-demo".to_string()]);
    }

    #[test]
    fn claude_code_approval_request_phrase_is_blocked() {
        let result = recognize_claude_code(&lines(&[
            "Plan approval request from teammate",
            "Press Enter to confirm",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
    }
}
