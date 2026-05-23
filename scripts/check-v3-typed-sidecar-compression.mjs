#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';
import { loadV3SemanticFacts, semanticFactDiagnostics } from './lib/v3_semantic_facts.mjs';

const BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const SIDECAR = '.missiond/v3/evidence/blueprint-notes.lisp';
const EMITTER = 'tools/missiond_lispc/bin/emit_json.ml';

function main() {
  const json = process.argv.includes('--json');
  const repo = process.cwd();
  const diagnostics = [];
  const blueprint = readBlueprintWithEvidenceSidecars(repo, BLUEPRINT);
  const sidecar = read(path.join(repo, SIDECAR), SIDECAR, diagnostics);
  const emitter = read(path.join(repo, EMITTER), EMITTER, diagnostics);
  const facts = loadV3SemanticFacts({ repoRoot: repo });
  diagnostics.push(...semanticFactDiagnostics(facts));

  requireText(BLUEPRINT, blueprint, diagnostics, [
    '(typed-subplane-contracts',
    ':semantic-ir-facts [contract_split control_plane_domain runtime_policy checker_registry',
    'production_consumer_boundary request_state_projection scanner_policy semantic_gate',
    '.missiond/v3/evidence/blueprint-notes.lisp',
  ]);
  requireText(SIDECAR, sidecar, diagnostics, [
    'note-001',
    'note-002',
    'note-003',
  ]);
  requireText(EMITTER, emitter, diagnostics, [
    'semantic_contract_split_facts',
    'semantic_control_plane_domain_fact',
    'semantic_typed_subplane_facts',
  ]);
  if (facts.contractSplits.size === 0) {
    diagnostics.push(diag(BLUEPRINT, 'compiled semantic IR must emit contract_split facts'));
  }
  if (facts.controlPlaneDomains.size === 0) {
    diagnostics.push(diag(BLUEPRINT, 'compiled semantic IR must emit control_plane_domain facts'));
  }
  if (facts.productionConsumerBoundaries.size === 0) {
    diagnostics.push(diag(BLUEPRINT, 'compiled semantic IR must emit production_consumer_boundary facts'));
  }
  if (facts.requestStateProjections.size === 0) {
    diagnostics.push(diag(BLUEPRINT, 'compiled semantic IR must emit request_state_projection facts'));
  }
  if (facts.scannerPolicies.size === 0) {
    diagnostics.push(diag(BLUEPRINT, 'compiled semantic IR must emit scanner_policy facts'));
  }
  if (facts.semanticGates.size === 0) {
    diagnostics.push(diag(BLUEPRINT, 'compiled semantic IR must emit semantic_gate facts'));
  }

  const result = {
    ok: diagnostics.length === 0,
    contractSplits: facts.contractSplits.size,
    controlPlaneDomains: facts.controlPlaneDomains.size,
    productionConsumerBoundaries: facts.productionConsumerBoundaries.size,
    requestStateProjections: facts.requestStateProjections.size,
    scannerPolicies: facts.scannerPolicies.size,
    semanticGates: facts.semanticGates.size,
    diagnostics,
  };
  if (json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('v3 typed sidecar compression check OK');
  else for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
  process.exit(result.ok ? 0 : 1);
}

function read(abs, rel, diagnostics) {
  try {
    return fs.readFileSync(abs, 'utf8');
  } catch (err) {
    diagnostics.push(diag(rel, `cannot read: ${err.message}`));
    return '';
  }
}

function requireText(file, source, diagnostics, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push(diag(file, `missing typed sidecar compression anchor: ${needle}`));
    }
  }
}

function diag(file, message) {
  return { file, line: 1, column: 1, code: 'V3_TYPED_SIDECAR_COMPRESSION', message };
}

main();
