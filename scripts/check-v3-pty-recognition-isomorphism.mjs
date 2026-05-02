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
    'recognize_claude_code',
    'fuse_with_session_state',
    'active_running_evidence',
    'screen_fused',
    'codex:status_indicator_widget',
    'gemini:loading_indicator_responding',
    'claude_code:active_spinner',
  ]);

  requireAll(diagnostics, files.ptySession, sources.ptySession, [
    'RecognitionUpdate(PtyRecognitionSnapshot)',
    'CodexCliStateParser::new()',
    'GeminiCliUpstreamStateParser::new()',
    'recognize_screen(engine, &last_lines, current_state)',
  ]);

  requireAll(diagnostics, files.ptyManager, sources.ptyManager, [
    'pub recognition: Option<PtyRecognitionSnapshot>',
    'SessionEvent::RecognitionUpdate(snapshot)',
    'session_state_snapshot',
  ]);

  requireAll(diagnostics, files.ptyLib, sources.ptyLib, [
    'mod pty_recognition;',
    'PtyRecognitionSnapshot',
    'recognize_screen',
  ]);

  requireAll(diagnostics, files.ptyHandler, sources.ptyHandler, [
    '"mission_pty_status"',
    'serde_json::to_value(&info)',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push({ file, message: `missing ${needle}` });
  }
}

main();
