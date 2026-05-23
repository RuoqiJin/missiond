#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const ROOT = process.cwd();
const args = new Set(process.argv.slice(2));
const json = args.has('--json');

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  workstationPoolEvidence: '.missiond/v3/evidence/workstation-pool.lisp',
  session: 'crates/missiond-pty/src/session.rs',
  recognition: 'crates/missiond-pty/src/pty_recognition.rs',
  genericCli: 'crates/missiond-daemon/src/slot_orchestrator/generic_cli.rs',
  orchestrator: 'crates/missiond-daemon/src/slot_orchestrator/mod.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  runtime: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  controlTree: 'crates/missiond-daemon/src/control_tree.rs',
  frontendBlueprint: '.missiond/frontend/board-blueprint.lisp',
};

function read(rel, resolved = false) {
  return resolved
    ? readBlueprintWithEvidenceSidecars(ROOT, rel)
    : fs.readFileSync(path.join(ROOT, rel), 'utf8');
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required text: ${needle}` });
    }
  }
}

function main() {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(FILES)) {
    try {
      sources[key] = read(rel, key === 'blueprint');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length === 0) {
    requireAll(diagnostics, FILES.blueprint, sources.blueprint, [
      'agent-cli-regression-policy',
      'node scripts/check-v3-agent-cli-regression.mjs',
      'Jarvis broad requests MUST pass the strict intent/plan gate',
      'agy-research',
      'codex-code-worker',
      'codex-review-worker',
      'Agy is the successor research CLI lane',
      'Codex code/review worker lanes are ordinary BoardTask candidates',
    ]);
    requireAll(diagnostics, FILES.workstationPoolEvidence, sources.workstationPoolEvidence, [
      'agy-research',
      'codex-code-worker',
      'codex-review-worker',
    ]);
    requireAll(diagnostics, FILES.session, sources.session, [
      'CliEngine::Agy',
      '"agy chat"',
      'Agy CLI command assembled',
      'AgyCliStateParser::new()',
      'mcp_servers.missiond.tools.mission_compute_slot.approval_mode="approve"',
      'mcp_servers.missiond.tools.mission_context_boot.approval_mode="approve"',
      'mcp_servers.missiond.tools.mission_context_slice.approval_mode="approve"',
      'mcp_servers.missiond.tools.mission_shared_memory.approval_mode="approve"',
      'mcp_servers.missiond.tools.mission_claim_status.approval_mode="approve"',
    ]);
    requireAll(diagnostics, FILES.recognition, sources.recognition, [
      'CliEngine::Agy => recognize_agy(lines)',
      'pub struct AgyCliStateParser',
      'fn recognize_agy',
      'agy_idle_screen_is_idle',
      'agy_feedback_prompt_after_answer_is_complete',
      'agy_auth_or_quota_error_is_blocked',
    ]);
    requireAll(diagnostics, FILES.genericCli, sources.genericCli, [
      'GenericCliSlotManager',
      'TextComplete',
      'reasoning_effort: req.reasoning_effort.clone()',
      'canonical_source_for_engine(self.engine)',
    ]);
    requireAll(diagnostics, FILES.orchestrator, sources.orchestrator, [
      'CliEngine::Agy => "agy_cli"',
      'chat_type_for_source',
      '"agy_cli"',
    ]);
    requireAll(diagnostics, FILES.main, sources.main, [
      'GenericCliSlotManager::new',
      'CliEngine::Agy',
      '"agy" | "agy-cli"',
      'reasoning_effort: worker.reasoning_effort.clone()',
      'search_enabled: worker.search_enabled',
    ]);
    requireAll(diagnostics, FILES.runtime, sources.runtime, [
      'workstation-pool must include a read-only Agy BoardTask worker',
      'workstation-pool must include at least one Codex non-master worker lane',
    ]);
    requireAll(diagnostics, FILES.controlTree, sources.controlTree, [
      'Agy,',
      'Self::Agy => "agy"',
      'Self::Agy => None',
    ]);
    requireAll(diagnostics, FILES.frontendBlueprint, sources.frontendBlueprint, [
      'jarvis-intent-draft',
      'jarvis-plan-draft',
      'jarvis-confirm-required',
      'jarvis-result-artifact',
    ]);
  }

  const result = { ok: diagnostics.length === 0, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 agent CLI regression check OK');
  } else {
    for (const diagnostic of diagnostics) {
      console.error(`${diagnostic.file}: ${diagnostic.message}`);
    }
    console.error(`v3 agent CLI regression check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

main();
