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
    pub selected_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

impl ProviderScreenIdentity {
    fn is_empty(&self) -> bool {
        self.cli_version.is_none()
            && self.account.is_none()
            && self.plan.is_none()
            && self.current_model.is_none()
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
    Regex::new(r"^(?P<cwd>(?:~|/[A-Za-z0-9_.-]+)/(?:[^\s|]+/?)+)$")
        .expect("valid agy cwd-only regex")
});

static AGY_CWD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?P<cwd>(?:~|/[A-Za-z0-9_.-]+)/(?:[^\s|]+/?)+)").expect("valid agy cwd regex")
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
            source: "screen_fallback".to_string(),
        }
    }

    fn with_phase(mut self, phase: impl Into<String>) -> Self {
        self.phase = Some(phase.into());
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
            Some("auth_missing" | "billing_or_account" | "usage_limit")
        )
}

pub fn recognize_screen(
    provider: CliEngine,
    lines: &[String],
    current_state: SessionState,
) -> PtyRecognitionSnapshot {
    let mut snapshot = match provider {
        CliEngine::Codex => recognize_codex(lines),
        CliEngine::Gemini => recognize_gemini(lines),
        CliEngine::Agy => recognize_agy(lines),
        CliEngine::ClaudeCode => recognize_claude_code(lines),
    };
    if snapshot.state == PtyCanonicalState::Unknown {
        snapshot = session_state_snapshot(provider, current_state);
    }
    fuse_with_session_state(provider, lines, current_state, snapshot)
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
        if let Some(active) = active_running_evidence(provider, lines) {
            return active;
        }
        return session_state_snapshot(provider, current_state);
    }
    snapshot
}

fn active_running_evidence(
    provider: CliEngine,
    lines: &[String],
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
                .with_source("screen_fused");
                snapshot = if let Some(tool) = extract_tool_name(lines) {
                    snapshot.with_tool(tool).with_phase("tool")
                } else {
                    snapshot.with_phase("thinking")
                };
                Some(snapshot)
            } else {
                None
            }
        }
        CliEngine::Codex => {
            if lower.contains("working (")
                || lower.contains(" esc to interrupt")
                || lower.contains("running command")
                || lower.contains("command running")
                || has_spinner(lines)
            {
                let mut snapshot = PtyRecognitionSnapshot::new(
                    CliEngine::Codex,
                    PtyCanonicalState::Running,
                    0.9,
                    "codex:status_indicator_widget",
                )
                .with_elapsed(elapsed)
                .with_source("screen_fused");
                if let Some(tool) = extract_tool_name(lines) {
                    snapshot = snapshot.with_tool(tool);
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
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let elapsed = extract_elapsed_secs(&text);

    if let Some((kind, reason)) = provider_unavailable_match(&lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Blocked,
            0.95,
            reason,
        )
        .with_blocked_kind(kind)
        .with_elapsed(elapsed)
        .with_source("provider_error_signature");
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
        .with_source("tui_source_signature");
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
        .with_elapsed(elapsed);
    }

    if lower.contains("working (")
        || lower.contains(" esc to interrupt")
        || lower.contains("reviewing approval request")
        || lower.contains("running command")
        || lower.contains("command running")
        || has_spinner(lines)
    {
        let mut snapshot = PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Running,
            0.9,
            "codex:status_indicator_widget",
        )
        .with_elapsed(elapsed);
        if let Some(tool) = extract_tool_name(lines) {
            snapshot = snapshot.with_tool(tool);
        }
        return snapshot;
    }

    if has_completion_line(lines) && has_idle_prompt(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::Codex,
            PtyCanonicalState::Complete,
            0.86,
            "codex:turn_complete_prompt_returned",
        )
        .with_elapsed(elapsed);
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
        );
    }

    PtyRecognitionSnapshot::new(
        CliEngine::Codex,
        PtyCanonicalState::Unknown,
        0.2,
        "codex:no_match",
    )
}

fn is_codex_approval_menu(lower: &str) -> bool {
    lower.contains("allow the ")
        && lower.contains(" mcp server to run tool")
        && lower.contains("allow for this session")
        && lower.contains("enter to submit")
        && lower.contains("esc to cancel")
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

    if let Some((kind, reason)) = provider_unavailable_match(&lower) {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Blocked,
            0.95,
            reason,
        )
        .with_blocked_kind(kind)
        .with_elapsed(elapsed)
        .with_source("provider_error_signature");
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
        .with_elapsed(elapsed);
    }

    if has_completion_line(lines) && has_idle_prompt(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Complete,
            0.86,
            "claude_code:turn_completion_verb",
        )
        .with_elapsed(elapsed);
    }

    if (lower.contains("auto mode on") || has_idle_prompt(lines)) && !current_activity {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Idle,
            0.9,
            "claude_code:prompt_idle",
        );
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
        .with_elapsed(elapsed);
        if let Some(tool) = extract_tool_name(lines) {
            snapshot = snapshot.with_tool(tool).with_phase("tool");
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
        if cleaned.is_empty()
            || lower.contains("scroll")
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
                if next.is_empty()
                    || next_lower.contains("scroll")
                    || next_lower.contains("pgup/pgdown")
                    || next_lower.contains("ctrl+end")
                    || next_lower.contains("ctrl+home")
                    || next_lower == "close"
                    || next_lower.contains("esc to cancel")
                    || parse_visible_range(&next).is_some()
                {
                    break;
                }
                if percent.is_none() {
                    percent = parse_percent(&next);
                } else if status.is_none()
                    && !is_agy_meter_bar(&next)
                    && extract_agy_model_from_line(&next).is_none()
                {
                    status = Some(next);
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
        if let Some(captures) = AGY_CWD_ONLY_RE.captures(&cleaned) {
            return captures
                .name("cwd")
                .map(|value| normalize_identity_value(value.as_str()));
        }
    }

    for line in lines {
        let cleaned = clean_agy_identity_line(line);
        let lower = cleaned.to_ascii_lowercase();
        if !(lower.contains("cwd") || lower.contains("directory") || lower.contains("project")) {
            continue;
        }
        if let Some(captures) = AGY_CWD_RE.captures(&cleaned) {
            return captures
                .name("cwd")
                .map(|value| normalize_identity_value(value.as_str()));
        }
    }

    None
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
            cleaned.starts_with('/')
                || cleaned.starts_with("> /")
                || cleaned.starts_with("› /")
                || cleaned.starts_with("❯ /")
        })
}

fn is_agy_pending_slash_command(lines: &[String]) -> bool {
    let recent = lines
        .iter()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(6)
        .collect::<Vec<_>>();
    let has_ready_footer = recent.iter().any(|line| {
        normalize_identity_value(&clean_agy_identity_line(line))
            .to_ascii_lowercase()
            .contains("? for shortcuts")
    });
    has_ready_footer
        && recent.iter().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("> /") || trimmed.starts_with("› /") || trimmed.starts_with("❯ /")
        })
}

fn is_agy_shell_prompt_after_exit(lines: &[String], lower: &str) -> bool {
    if !lower.contains("resume with:") && !lower.contains("resume: agy --conversation=") {
        return false;
    }
    lines.iter().rev().take(8).any(|line| {
        SHELL_PROMPT_RE.is_match(&normalize_identity_value(&clean_agy_identity_line(line)))
    })
}

fn has_active_claude_spinner(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        let trimmed = line.trim_start();
        let starts_with_spinner = trimmed
            .chars()
            .next()
            .is_some_and(|c| "·✻✽✶✳✢*⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".contains(c));
        starts_with_spinner && (trimmed.contains("...") || trimmed.contains('…'))
    })
}

fn has_current_claude_activity_line(lines: &[String]) -> bool {
    lines.iter().rev().take(6).any(|line| {
        let trimmed = line.trim_start();
        let starts_with_spinner = trimmed
            .chars()
            .next()
            .is_some_and(|c| "·✻✽✶✳✢*⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".contains(c));
        starts_with_spinner
            && (trimmed.contains("esc to interrupt")
                || trimmed.contains("almost done thinking")
                || trimmed.contains("thinking with"))
    })
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
    lines.iter().rev().take(8).any(|line| {
        let trimmed = line
            .trim_start()
            .trim_start_matches(|c: char| "·✻✽✶✳✢*⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ".contains(c));
        trimmed.starts_with("Worked for")
            || trimmed.starts_with("Churned for")
            || trimmed.starts_with("Baked for")
            || trimmed.starts_with("Cooked for")
    })
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

    fn lines(input: &[&str]) -> Vec<String> {
        input.iter().map(|line| line.to_string()).collect()
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
    fn claude_completion_and_prompt_is_complete() {
        let result = recognize_claude_code(&lines(&[
            "* Worked for 10s",
            "› auto mode on (shift+tab to cycle)",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Complete);
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
    fn claude_code_approval_request_phrase_is_blocked() {
        let result = recognize_claude_code(&lines(&[
            "Plan approval request from teammate",
            "Press Enter to confirm",
        ]));
        assert_eq!(result.state, PtyCanonicalState::Blocked);
    }
}
