#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-pty-recognition-isomorphism.mjs [--json]

Checks that V3 upstream PTY signatures and MissionD runtime recognition agree:
  - V3 names Codex, Gemini, and ClaudeCode upstream state surfaces.
  - missiond-pty exposes a provider-aware recognition snapshot.
  - Codex and Gemini no longer fall back to the ClaudeCode state parser.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  ptyRecognition: 'crates/missiond-pty/src/pty_recognition.rs',
  ptySession: 'crates/missiond-pty/src/session.rs',
  ptyManager: 'crates/missiond-pty/src/manager.rs',
  ptyLib: 'crates/missiond-pty/src/lib.rs',
  ptyHandler: 'crates/missiond-daemon/src/handlers/compute/pty.rs',
  ptyEventWorker: 'crates/missiond-daemon/src/workers/local/pty_event_worker.rs',
  masterControl: 'crates/missiond-daemon/src/engine/master_control.rs',
  ptyCleanup: 'scripts/cleanup-pty-diagnostics.mjs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  const diagnostics = checkFiles(process.cwd(), DEFAULT_FILES);
  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 PTY recognition isomorphism check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 PTY recognition check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    try {
      sources[key] =
        key === 'blueprint'
          ? readBlueprintWithEvidenceSidecars(root, rel)
          : fs.readFileSync(path.join(root, rel), 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    '(upstream-pty-signatures',
    '(provider codex-cli',
    'https://github.com/openai/codex',
    'codex-rs/tui/src/status_indicator_widget.rs',
    'codex-rs/tui/src/chatwidget.rs',
    '(provider gemini-cli',
    'https://github.com/google-gemini/gemini-cli',
    'packages/cli/src/ui/types.ts',
    'packages/cli/src/ui/components/LoadingIndicator.tsx',
    '(provider claude-code',
    '/Users/jinchen/Downloads/claudecode/claudecode',
    'PtyRecognitionSnapshot',
    'mission_pty_status',
    'recognize_screen MUST fuse SessionState with screen heuristics',
    'screen_fused',
    'explicit Confirming SessionState always preserves Blocked',
    'Exited/terminal SessionState overrides stale running screen evidence',
    'Codex MCP approval menus',
    'human-like keyboard navigation',
    'recognize_claude_code Blocked MUST require explicit confirmation/model-picker UI',
    'Confirming is an active turn state, not a TextOutput::Complete boundary',
    'bypass permissions on',
    'MUST NOT trigger Blocked on Idle or completed screens',
    'ClaudeCode worker MCP reconnect MUST follow `/mcp` -> Enter -> ArrowDown until missiond -> Enter -> Enter using arrow-key keystrokes only',
    'Session::mcp_reconnect_sequence MUST project from it',
    'master_control MUST file a durable claude_code_mcp_missing incident',
    'claude_code_mcp_reconnect_failed',
    '(claude-code-mcp-recovery',
    ':reconnect-keystrokes ["/mcp" "<enter>" "<arrow-down>*N" "<enter>" "<enter>"]',
    ':forbid-numeric-shortcut true',
    ':missing-incident-kind "claude_code_mcp_missing"',
    ':reconnect-failed-incident-kind "claude_code_mcp_reconnect_failed"',
    ':reconnect-budget-attempts 1',
    ':wake-resident-master true',
    ':pty-retention',
    ':ttl-days 1',
    'PTY content is transient diagnostic evidence only',
    'crates/missiond-pty/src/session.rs::Session::mcp_reconnect_sequence',
    'crates/missiond-pty/src/session.rs::PTYSession::mcp_reconnect',
    'crates/missiond-pty/src/manager.rs::PTYManager::mcp_reconnect',
    'crates/missiond-daemon/src/workers/local/pty_event_worker.rs::handle_mcp_tool_error',
    'crates/missiond-daemon/src/engine/master_control.rs::spawn_incident_event_sub',
    'node scripts/check-v3-pty-recognition-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.ptyRecognition, sources.ptyRecognition, [
    'PtyRecognitionSnapshot',
    'PtyCanonicalState',
    'CodexCliStateParser',
    'GeminiCliUpstreamStateParser',
    'recognize_screen',
    'recognize_codex',
    'recognize_gemini',
    'recognize_agy',
    'recognize_claude_code',
    'AgyCliStateParser',
    'fuse_with_session_state',
    'SessionState::Exited | SessionState::Error',
    'active_running_evidence',
    'screen_fused',
    'is_codex_approval_menu',
    'codex:mcp_approval_menu',
    'tui_source_signature',
    'codex:status_indicator_widget',
    'gemini:loading_indicator_responding',
    'agy:feedback_prompt_after_complete',
    'claude_code:active_spinner',
    '"do you want to proceed"',
    '"do you want to make this edit"',
    '"do you want to allow"',
    '"select model"',
    '"approval request"',
  ]);

  forbidAll(diagnostics, files.ptyRecognition, sources.ptyRecognition, [
    // Generic single-word matchers MUST NOT reappear: they false-positive on
    // task-brief prose and the `bypass permissions on` composer footer.
    'lower.contains("approval")',
    'lower.contains("permission")',
    'lower.contains("permissions")',
  ]);

  requireAll(diagnostics, files.ptySession, sources.ptySession, [
    'RecognitionUpdate(PtyRecognitionSnapshot)',
    'CodexCliStateParser::new()',
    'GeminiCliUpstreamStateParser::new()',
    'AgyCliStateParser::new()',
    'recognize_screen(engine, &last_lines, current_state)',
    'navigate_to_option_then_enter',
    'confirmation_navigation_sequence',
    'confirmation_navigation_uses_arrow_keys_not_numeric_shortcuts',
    'screen_is_blocked_confirmation',
    'fn confirming_is_active_turn_not_completion_boundary',
    'SessionState::Confirming.is_processing()',
    'reject_option_index',
    // ClaudeCode MCP arrow-key reconnect ritual SSOT — both the static
    // byte-template helper and the live runtime method must exist, plus
    // the McpReconnectOutcome carrier the IncidentEvent subscriber wakes
    // on.
    'mcp_reconnect_sequence',
    'pub fn mcp_reconnect_sequence',
    'pub async fn mcp_reconnect',
    'pub struct McpReconnectOutcome',
    '"/mcp"',
    '"\\x1b[B"',
  ]);
  forbidAll(diagnostics, files.ptySession, sources.ptySession, [
    'press_digit_then_enter',
    'std::char::from_digit',
    'write_all(b"2',
    'write_all(b"3',
    // The arrow-key reconnect ritual is the only sanctioned path; no
    // numeric shortcut helper may resurface under a renamed identifier.
    'fn reconnect_via_digit',
    'mcp_numeric_shortcut',
  ]);

  requireAll(diagnostics, files.ptyManager, sources.ptyManager, [
    'pub recognition: Option<PtyRecognitionSnapshot>',
    'SessionEvent::RecognitionUpdate(snapshot)',
    'session_state_snapshot',
    'normalize_agent_info',
    'exited_status_overrides_stale_running_recognition',
    // ClaudeCode MCP arrow-key reconnect ritual is exposed on the manager
    // so callers (e.g. pty_event_worker) never reach into PTYSession
    // directly to schedule the budget-1 attempt.
    'pub async fn mcp_reconnect',
    'McpReconnectOutcome',
  ]);

  requireAll(diagnostics, files.ptyLib, sources.ptyLib, [
    'mod pty_recognition;',
    'PtyRecognitionSnapshot',
    'recognize_screen',
    // McpReconnectOutcome must be re-exported so the daemon's
    // pty_event_worker can carry it into the follow-up incident payload.
    'McpReconnectOutcome',
  ]);

  requireAll(diagnostics, files.ptyHandler, sources.ptyHandler, [
    '"mission_pty_status"',
    'serde_json::to_value(&info)',
    'confirm_response_from_value',
    'mission_pty_confirm must not send arbitrary raw text',
  ]);
  forbidAll(diagnostics, files.ptyHandler, sources.ptyHandler, [
    'format!("{}\\r", s)',
    'directly maps',
    'full chain from keystroke',
  ]);

  // ClaudeCode MCP-missing → arrow-key reconnect → optional reconnect-failed
  // pipeline is the trigger for the IncidentEvent wakeup. The pty_event_worker
  // is the single producer; if its tagging or budget-1 invariant regresses,
  // the resident master no longer wakes automatically and the user has to
  // notice the failure manually.
  requireAll(diagnostics, files.ptyEventWorker, sources.ptyEventWorker, [
    'CLAUDE_CODE_MCP_MISSING_INCIDENT_KIND',
    'CLAUDE_CODE_MCP_RECONNECT_FAILED_INCIDENT_KIND',
    'pty.mcp_reconnect',
    'missiond_core::CliEngine::ClaudeCode',
    'screen_excerpt',
  ]);

  // Master_control is the single consumer of the MCP-missing /
  // reconnect-failed incident kinds. The IncidentEvent live subscription
  // and the kind-based filter are pinned together so a regression that
  // drops one drops the other and is caught here.
  requireAll(diagnostics, files.masterControl, sources.masterControl, [
    'IncidentEvent',
    'MASTER_INCIDENT_SUBSCRIPTION',
    'spawn_incident_event_sub',
    'should_wake_for_incident_event',
    'CLAUDE_CODE_MCP_MISSING_INCIDENT_KIND',
    'CLAUDE_CODE_MCP_RECONNECT_FAILED_INCIDENT_KIND',
  ]);

  requireAll(diagnostics, files.ptyCleanup, sources.ptyCleanup, [
    'cleanup-pty-diagnostics',
    'retention_days',
    'pty-.*\\.log',
    'screenshots',
    'conversation_messages',
    'expired_pty_sessions',
    'chat_type',
    'VACUUM (FULL, ANALYZE)',
    '--days',
    '--db',
    '--skip-files',
    '--skip-db',
    '--apply',
    '--manifest',
    'manifest_path',
    'fs.writeFileSync(manifestPath',
    'isPtyDiagnosticFile',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push({ file, message: `missing ${needle}` });
  }
}

function forbidAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle))
      diagnostics.push({ file, message: `forbidden substring present: ${needle}` });
  }
}

main();
