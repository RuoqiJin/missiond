#!/usr/bin/env node

import fs from 'node:fs';
import { runLispc } from './lib/ocaml_lispc.mjs';

const GENOME_DIR = '.missiond/v3/genome';

const EXPECTED_RECEPTORS = [
  'BoardEvent::TaskCreated',
  'BoardEvent::Updated(status=open)',
  'BoardEvent::StatusChanged(new_status=open)',
  'SlotEvent::BecameIdle',
  'SystemTick::Autopilot60s',
  'Command::NotifyAutopilotDispatch',
];

const EXPECTED_EFFECTS = [
  'NotifyAutopilotDispatch',
  'RunAutopilotTick',
  'RunBoardDispatch',
  'PublishBoardEvent',
  'AddBoardTaskNote',
  'UpdateBoardTask',
  'Log',
  'Metric',
  'Noop',
];

const RUST_AUTOPILOT_TOKENS = [
  'pub struct AutopilotCell',
  'CMD_NOTIFY_AUTOPILOT_DISPATCH',
  'CMD_RUN_AUTOPILOT_TICK',
  'CMD_RUN_BOARD_DISPATCH',
  'BOARD_TASK_CREATED',
  'BOARD_UPDATED_OPEN',
  'BOARD_STATUS_CHANGED_OPEN',
  'SLOT_BECAME_IDLE',
  'SYSTEM_TICK_AUTOPILOT_60S',
  'COMMAND_NOTIFY_AUTOPILOT_DISPATCH',
];

const DAEMON_TOKENS = [
  'process_autopilot_board_event',
  'process_autopilot_slot_event',
  'run_autopilot_tick',
  'run_autopilot_board_dispatch',
  'AutopilotEffectInterpreter',
  'publish_incident',
];

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const diagnostics = [];
  const emit = runLispc(['emit-genomes', '--genome-dir', GENOME_DIR]);
  if (!emit.ok || !emit.compiled) {
    diagnostics.push(diag('tools/missiond_lispc/bin/genome_schema.ml', 'GENOME_EMIT_FAILED', 'emit-genomes failed'));
  }
  const genomes = emit.compiled?.payload?.genomes ?? [];
  const autopilot = genomes.find((genome) => genome.id === 'missiond-autopilot');
  const organ = autopilot?.organs?.find((row) => row.id === 'autopilot');
  if (!autopilot) diagnostics.push(diag('.missiond/v3/genome/autopilot.lisp', 'AUTOPILOT_GENOME_MISSING', 'missing missiond-autopilot genome'));
  if (!organ) diagnostics.push(diag('.missiond/v3/genome/autopilot.lisp', 'AUTOPILOT_ORGAN_MISSING', 'missing autopilot organ'));

  const tissues = organ?.tissues ?? [];
  const receptors = new Set(tissues.flatMap((tissue) => tissue.receptors ?? []));
  const effects = new Set(tissues.flatMap((tissue) => tissue.allow_effects ?? []));
  for (const receptor of EXPECTED_RECEPTORS) {
    if (!receptors.has(receptor)) {
      diagnostics.push(diag('.missiond/v3/genome/autopilot.lisp', 'AUTOPILOT_RECEPTOR_MISSING', `missing receptor ${receptor}`));
    }
  }
  for (const effect of EXPECTED_EFFECTS) {
    if (!effects.has(effect)) {
      diagnostics.push(diag('.missiond/v3/genome/autopilot.lisp', 'AUTOPILOT_EFFECT_MISSING', `missing allowed effect ${effect}`));
    }
  }

  const rustAutopilot = read('crates/missiond-organism-runtime/src/autopilot.rs');
  for (const token of RUST_AUTOPILOT_TOKENS) {
    if (!rustAutopilot.includes(token)) {
      diagnostics.push(diag('crates/missiond-organism-runtime/src/autopilot.rs', 'RUST_AUTOPILOT_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
    }
  }

  const subscribers = read('crates/missiond-daemon/src/bus/v2_subscribers.rs');
  const main = read('crates/missiond-daemon/src/main.rs');
  const organRuntime = read('crates/missiond-daemon/src/organism/autopilot_organ.rs');
  const daemonSources = `${subscribers}\n${main}\n${organRuntime}`;
  for (const token of DAEMON_TOKENS) {
    if (!daemonSources.includes(token)) {
      diagnostics.push(diag('crates/missiond-daemon/src/organism/autopilot_organ.rs', 'DAEMON_AUTOPILOT_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
    }
  }

  const result = {
    ok: diagnostics.length === 0,
    diagnostics,
    receptors: Array.from(receptors).sort(),
    effects: Array.from(effects).sort(),
  };
  if (opts.json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('V3 autopilot genome isomorphism OK');
  else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.code}: ${d.message}`);
    console.error('V3 autopilot genome isomorphism FAILED');
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false };
  for (const arg of argv) {
    if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/check-v3-autopilot-genome-isomorphism.mjs [--json]');
      process.exit(0);
    } else {
      console.error(`unknown argument: ${arg}`);
      process.exit(2);
    }
  }
  return opts;
}

function read(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return '';
  }
}

function diag(file, code, message) {
  return { file, line: 1, column: 1, code, message, path: '' };
}

main();
