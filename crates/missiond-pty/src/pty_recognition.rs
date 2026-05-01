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
    snapshot
}

fn recognize_codex(lines: &[String]) -> PtyRecognitionSnapshot {
    let text = joined_text(lines);
    let lower = text.to_ascii_lowercase();
    let elapsed = extract_elapsed_secs(&text);

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

    if lower.contains("enter to confirm")
        || lower.contains("do you want to proceed")
        || lower.contains("select model")
        || lower.contains("approval")
        || lower.contains("permission")
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
}
