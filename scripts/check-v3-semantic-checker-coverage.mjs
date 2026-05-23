#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { loadV3SemanticFacts, semanticFactDiagnostics } from './lib/v3_semantic_facts.mjs';

const REQUIRED_RUNTIME_POLICIES = [
  'autopilot-policy',
  'cascade-policy',
  'flow-runtime-policy',
  'compute-runtime-policy',
  'minimax-runtime-policy',
  'router-runtime-policy',
  'project-registry-policy',
  'capability-governance-policy',
  'memory-kb-policy',
  'conversation-ingestion-policy',
  'learning-engine-policy',
];

const REQUIRED_HELPER_ANCHORS = [
  'compiledRuntimePolicyMap',
  'compiledCheckerRegistryMap',
  'compiledContractSplitMap',
  'compiledControlPlaneDomainMap',
  'loadV3SemanticFacts',
];

function main() {
  const json = process.argv.includes('--json');
  const repo = process.cwd();
  const diagnostics = [];
  const facts = loadV3SemanticFacts({ repoRoot: repo });
  diagnostics.push(...semanticFactDiagnostics(facts));

  for (const id of REQUIRED_RUNTIME_POLICIES) {
    if (!facts.runtimePolicies.has(id)) {
      diagnostics.push(diag('.missiond/v3/missiond-blueprint.lisp', `compiled semantic IR missing runtime_policy fact: ${id}`));
    }
  }
  const checker = facts.checkerRegistry.get('v3-compression-contract');
  if (!checker || !checker.checks.includes('node scripts/check-v3-final-convergence.mjs')) {
    diagnostics.push(diag('.missiond/v3/missiond-blueprint.lisp', 'compiled checker registry missing final convergence command'));
  }
  if (facts.sourceUnits.size < 2) {
    diagnostics.push(diag('.missiond/v3/missiond-blueprint.lisp', 'compiled source_units must include root plus compiler-active shards'));
  }

  const helperSource = read(path.join(repo, 'scripts/lib/v3_semantic_facts.mjs'), diagnostics);
  const contractSource = read(path.join(repo, 'scripts/lib/v3_compiled_contract.mjs'), diagnostics);
  for (const needle of REQUIRED_HELPER_ANCHORS) {
    if (!helperSource.includes(needle) && !contractSource.includes(needle)) {
      diagnostics.push(diag('scripts/lib/v3_semantic_facts.mjs', `missing shared semantic checker helper anchor: ${needle}`));
    }
  }
  const rawSemanticTokenPins = countRawSemanticTokenPins(repo, diagnostics);
  if (rawSemanticTokenPins > 0) {
    diagnostics.push(diag('scripts', `raw semantic token pins must use compiled semantic facts/helper readers; found ${rawSemanticTokenPins}`));
  }
  const rawParserImports = countRawParserImports(repo, diagnostics);
  if (rawParserImports > 0) {
    diagnostics.push(diag('scripts', `V3 checkers must not import scripts/lib/missiond_lisp.mjs directly; found ${rawParserImports}`));
  }

  const result = {
    ok: diagnostics.length === 0,
    runtimePolicies: facts.runtimePolicies.size,
    checkerRegistry: facts.checkerRegistry.size,
    contractSplits: facts.contractSplits.size,
    controlPlaneDomains: facts.controlPlaneDomains.size,
    sourceUnits: facts.sourceUnits.size,
    rawSemanticTokenPins,
    rawParserImports,
    diagnostics,
  };
  if (json) console.log(JSON.stringify(result, null, 2));
  else if (result.ok) console.log('v3 semantic checker coverage check OK');
  else for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
  process.exit(result.ok ? 0 : 1);
}

function read(file, diagnostics) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch (err) {
    diagnostics.push(diag(file, `cannot read: ${err.message}`));
    return '';
  }
}

function countRawParserImports(repo, diagnostics) {
  const scriptsDir = path.join(repo, 'scripts');
  let files;
  try {
    files = fs.readdirSync(scriptsDir)
      .filter((name) => /^check-v3-.*\.mjs$/.test(name))
      .map((name) => path.join(scriptsDir, name));
  } catch (err) {
    diagnostics.push(diag('scripts', `cannot scan checkers: ${err.message}`));
    return 0;
  }
  let count = 0;
  const importRe = /(?:from\s+['"]\.\/lib\/missiond_lisp\.mjs['"]|import\s*\(\s*['"]\.\/lib\/missiond_lisp\.mjs['"]\s*\))/;
  for (const file of files) {
    const source = read(file, diagnostics);
    if (importRe.test(source)) count += 1;
  }
  return count;
}

function countRawSemanticTokenPins(repo, diagnostics) {
  const scriptsDir = path.join(repo, 'scripts');
  let files;
  try {
    files = fs.readdirSync(scriptsDir)
      .filter((name) => /^check-v3-.*\.mjs$/.test(name))
      .map((name) => path.join(scriptsDir, name));
  } catch (err) {
    diagnostics.push(diag('scripts', `cannot scan checkers: ${err.message}`));
    return 0;
  }
  let count = 0;
  for (const file of files) {
    const source = read(file, diagnostics);
    const directBlueprintRead = /readFileSync\([^)]*(BLUEPRINT|missiond-blueprint\.lisp)/.test(source);
    const hasCompiledReader = /loadCompiledV3Contract|loadResolvedV3Contract|readBlueprintWithEvidenceSidecars|loadV3SemanticFacts/.test(source);
    if (directBlueprintRead && !hasCompiledReader) count += 1;
  }
  return count;
}

function diag(file, message) {
  return { file, line: 1, column: 1, code: 'V3_SEMANTIC_CHECKER_COVERAGE', message };
}

main();
