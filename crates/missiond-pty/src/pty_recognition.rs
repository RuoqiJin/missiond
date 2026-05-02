//! Provider-aware PTY recognition derived from upstream CLI TUI state surfaces.
//!
//! This module is intentionally local to `missiond-pty`: MissionD owns the
//! orchestration semantics even when the terminal UI belongs to Codex, Gemini,
//! or Claude Code. The upstream projects remain the evidence source; this code
//! turns visible PTY text into a stable MissionD recognition snapshot.

use missiond_shared::CliEngine;
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
    pub source: String,
}

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

pub fn recognize_screen(
    provider: CliEngine,
    lines: &[String],
    current_state: SessionState,
) -> PtyRecognitionSnapshot {
    let mut snapshot = match provider {
        CliEngine::Codex => recognize_codex(lines),
        CliEngine::Gemini => recognize_gemini(lines),
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
    match provider {
        CliEngine::ClaudeCode => {
            if lower.contains("esc to interrupt")
                || lower.contains("almost done thinking")
                || lower.contains("thinking with")
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
    }
}

fn recognize_codex(lines: &[String]) -> PtyRecognitionSnapshot {
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let elapsed = extract_elapsed_secs(&text);

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

fn recognize_claude_code(lines: &[String]) -> PtyRecognitionSnapshot {
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let elapsed = extract_elapsed_secs(&text);

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

    if lower.contains("esc to interrupt")
        || lower.contains("almost done thinking")
        || lower.contains("thinking with")
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

    if has_completion_line(lines) && has_idle_prompt(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Complete,
            0.86,
            "claude_code:turn_completion_verb",
        )
        .with_elapsed(elapsed);
    }

    if lower.contains("auto mode on") || has_idle_prompt(lines) {
        return PtyRecognitionSnapshot::new(
            CliEngine::ClaudeCode,
            PtyCanonicalState::Idle,
            0.9,
            "claude_code:prompt_idle",
        );
    }

    PtyRecognitionSnapshot::new(
        CliEngine::ClaudeCode,
        PtyCanonicalState::Unknown,
        0.2,
        "claude_code:no_match",
    )
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
    lines.iter().any(|line| {
        line.chars().any(|c| {
            matches!(
                c,
                '\u{2800}'..='\u{28FF}' | '◐' | '◑' | '◒' | '◓' | '◴' | '◵' | '◶' | '◷'
            )
        })
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
