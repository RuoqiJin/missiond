#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  head,
  isList,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';
import { maybeRunLispc } from './lib/ocaml_lispc.mjs';
import { EXPECTED_SURFACES } from './check-v3-code-isomorphism-complete.mjs';
import {
  compiledSurfaceIds,
  loadCompiledV3Contract,
} from './lib/v3_compiled_contract.mjs';
import { readBlueprintResolvedSource } from './lib/v3_blueprint_contract_source.mjs';

const BLUEPRINT_PATH = '.missiond/v3/missiond-blueprint.lisp';
const CHECK_COMMAND = 'node scripts/check-v3-pillar-flow-schema.mjs';

const usage = `Usage:
  node scripts/check-v3-pillar-flow-schema.mjs [--json] [--dry-fixture]
    [--blueprint <path>] [--engine=auto|js|ocaml]

Validates V3's pillar/function flow map:
  - Every code-aligned implementation surface is mapped by exactly one function.
  - Every function lives inside a pillar and declares :entry, :core, and :egress.
  - :core is an ordered list of (step sN :logic "...") forms with no gaps.
  - compression-contract :checks pins this checker.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    runDryFixture(opts);
    return;
  }
  const compiled = opts.engine === 'js'
    ? null
    : loadCompiledV3Contract({ blueprint: opts.blueprint, semanticIr: true });
  const expectedSurfaces = compiled ? compiledSurfaceIds(compiled) : EXPECTED_SURFACES;
  if (opts.engine !== 'js' && (!compiled?.ok || expectedSurfaces.length === 0)) {
    const result = {
      ok: false,
      engine: {
        requested: opts.engine,
        selected: 'compiled-contract',
      },
      diagnostics: compiled?.diagnostics ?? [{
        file: opts.blueprint,
        line: 1,
        column: 1,
        message: 'missiond-lispc emit-v3/emit-semantic-ir did not return surfaces',
      }],
    };
    if (opts.json) console.log(JSON.stringify(result, null, 2));
    else {
      for (const d of result.diagnostics) {
        console.error(`${d.file}:${d.line ?? 1}:${d.column ?? 1}: ${d.message}`);
      }
      console.error('v3 pillar-flow schema FAILED (compiled contract)');
    }
    process.exit(1);
  }
  const ocamlAttempt = maybeRunLispc([
    'check-v3',
    '--blueprint',
    opts.blueprint,
    '--expected-surfaces',
    expectedSurfaces.join(','),
  ], { engine: opts.engine });
  if (ocamlAttempt.mode === 'ocaml' || ocamlAttempt.mode === 'invalid') {
    const result = normalizeOcamlResult(ocamlAttempt.result, opts.engine);
    if (opts.json) {
      console.log(JSON.stringify(result, null, 2));
    } else if (result.ok) {
      console.log('v3 pillar-flow schema OK (ocaml engine)');
    } else {
      for (const d of result.diagnostics ?? []) {
        console.error(`${d.file}:${d.line ?? 1}:${d.column ?? 1}: ${d.message}`);
      }
      console.error('v3 pillar-flow schema FAILED (ocaml engine)');
    }
    process.exit(result.ok ? 0 : 1);
  }
  let source;
  try {
    source = readBlueprintResolvedSource(process.cwd(), opts.blueprint);
  } catch (err) {
    const result = {
      ok: false,
      engine: {
        requested: opts.engine,
        selected: 'js',
        fallback_reason: ocamlAttempt.mode === 'js-fallback'
          ? (ocamlAttempt.result?.diagnostics?.[0]?.message ?? 'OCaml engine unavailable')
          : null,
      },
      diagnostics: [{
        file: opts.blueprint,
        line: 1,
        column: 1,
        message: `cannot load resolved V3 blueprint: ${err.message}`,
      }],
    };
    if (opts.json) console.log(JSON.stringify(result, null, 2));
    else console.error(`${opts.blueprint}:1:1: ${result.diagnostics[0].message}`);
    process.exit(1);
  }
  const result = validatePillarFlowSource(source, opts.blueprint, { expectedSurfaces });
  result.engine = {
    requested: opts.engine,
    selected: 'js',
    fallback_reason: ocamlAttempt.mode === 'js-fallback'
      ? (ocamlAttempt.result?.diagnostics?.[0]?.message ?? 'OCaml engine unavailable')
      : null,
  };
  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(
      `v3 pillar-flow schema OK (${result.pillars.length} pillars, ${result.functions.length} functions)`,
    );
  } else {
    for (const d of result.diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.message}`);
    }
    console.error('v3 pillar-flow schema FAILED');
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, dryFixture: false, blueprint: BLUEPRINT_PATH, engine: 'ocaml' };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--blueprint') {
      opts.blueprint = argv[++i] ?? fail('--blueprint requires a value');
    } else if (arg.startsWith('--blueprint=')) {
      opts.blueprint = arg.slice('--blueprint='.length);
    } else if (arg === '--engine') {
      opts.engine = argv[++i] ?? fail('--engine requires a value');
    } else if (arg.startsWith('--engine=')) {
      opts.engine = arg.slice('--engine='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!['auto', 'js', 'ocaml'].includes(opts.engine)) fail(`unknown engine: ${opts.engine}`);
  return opts;
}

function normalizeOcamlResult(result, requested) {
  return {
    ok: result?.ok === true,
    engine: {
      requested,
      selected: 'ocaml',
      unavailable: result?.unavailable === true,
    },
    diagnostics: result?.diagnostics ?? [],
    toolchain: result?.toolchain ?? null,
    raw: result ?? null,
  };
}

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

export function validatePillarFlowSource(source, file = BLUEPRINT_PATH, {
  expectedSurfaces = EXPECTED_SURFACES,
} = {}) {
  const diagnostics = [];
  let forms;
  try {
    forms = parseLisp(source, file);
  } catch (err) {
    return {
      ok: false,
      pillars: [],
      functions: [],
      diagnostics: [{
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        message: err.message,
      }],
    };
  }

  const root = forms.find((form) => isList(form) && head(form) === 'missiond-blueprint');
  if (!root) {
    diagnostics.push(diag(file, { line: 1, column: 1 }, 'missing (missiond-blueprint ...) root'));
    return { ok: false, pillars: [], functions: [], diagnostics };
  }

  const implementationMap = childSection(root, 'implementation-map');
  if (!implementationMap) {
    diagnostics.push(diag(file, root.loc, 'missing (implementation-map ...) section'));
    return { ok: false, pillars: [], functions: [], diagnostics };
  }
  const declaredSurfaces = new Set(
    implementationMap.children
      .filter((node) => isList(node) && head(node) === 'surface')
      .map((node) => nodeText(node.children[1]))
      .filter(Boolean),
  );

  const flowMap = childSection(root, 'pillar-flow-map');
  if (!flowMap) {
    diagnostics.push(diag(file, root.loc, 'missing (pillar-flow-map ...) section'));
    return { ok: false, pillars: [], functions: [], diagnostics };
  }

  const pillars = [];
  const functions = [];
  const surfaceToFunctions = new Map();
  for (const pillar of flowMap.children.filter((node) => isList(node) && head(node) === 'pillar')) {
    const pillarId = nodeText(pillar.children[1]);
    if (!pillarId) {
      diagnostics.push(diag(file, pillar.loc, 'pillar must declare an id'));
      continue;
    }
    pillars.push(pillarId);
    const functionForms = pillar.children.filter((node) => isList(node) && head(node) === 'function');
    if (functionForms.length === 0) {
      diagnostics.push(diag(file, pillar.loc, `pillar "${pillarId}" must declare at least one function`));
    }
    for (const fn of functionForms) {
      const fnId = nodeText(fn.children[1]);
      const props = readKeywordProps(fn, { start: 2 });
      const surface = nodeText(props[':surface']?.value);
      const record = { pillar: pillarId, function: fnId, surface };
      functions.push(record);
      validateFunctionShape(diagnostics, file, pillarId, fnId, fn, props);
      if (!surface) {
        diagnostics.push(diag(file, fn.loc, `function "${fnId ?? '<missing>'}" must declare :surface`));
        continue;
      }
      if (!declaredSurfaces.has(surface)) {
        diagnostics.push(diag(file, fn.loc, `function "${fnId}" references unknown surface "${surface}"`));
      }
      const refs = surfaceToFunctions.get(surface) ?? [];
      refs.push(`${pillarId}/${fnId}`);
      surfaceToFunctions.set(surface, refs);
    }
  }

  for (const expected of expectedSurfaces) {
    const refs = surfaceToFunctions.get(expected) ?? [];
    if (refs.length === 0) {
      diagnostics.push(diag(file, flowMap.loc, `surface "${expected}" is not mapped by pillar-flow-map`));
    } else if (refs.length > 1) {
      diagnostics.push(diag(file, flowMap.loc, `surface "${expected}" is mapped by multiple functions: ${refs.join(', ')}`));
    }
  }

  const compressionContract = childSection(root, 'compression-contract');
  if (!compressionContract) {
    diagnostics.push(diag(file, root.loc, 'missing (compression-contract ...) section'));
  } else {
    const props = readKeywordProps(compressionContract, { start: 1 });
    const checks = listTexts(props[':checks']?.value);
    if (!checks.includes(CHECK_COMMAND)) {
      diagnostics.push(diag(file, compressionContract.loc, `compression-contract :checks must include "${CHECK_COMMAND}"`));
    }
  }

  return {
    ok: diagnostics.length === 0,
    pillars,
    functions,
    diagnostics,
  };
}

function validateFunctionShape(diagnostics, file, pillarId, fnId, fn, props) {
  if (!fnId) {
    diagnostics.push(diag(file, fn.loc, `function in pillar "${pillarId}" must declare an id`));
  }

  const entry = props[':entry']?.value;
  const core = props[':core']?.value;
  const egress = props[':egress']?.value;
  if (listTexts(entry).length === 0) {
    diagnostics.push(diag(file, fn.loc, `function "${fnId}" must declare non-empty :entry [...]`));
  }
  if (listTexts(egress).length === 0) {
    diagnostics.push(diag(file, fn.loc, `function "${fnId}" must declare non-empty :egress [...]`));
  }
  if (!core || !isList(core)) {
    diagnostics.push(diag(file, fn.loc, `function "${fnId}" must declare :core ((step s1 ...) ...)`));
    return;
  }
  const steps = core.children.filter((node) => isList(node) && head(node) === 'step');
  if (steps.length === 0) {
    diagnostics.push(diag(file, core.loc, `function "${fnId}" :core must contain at least one (step ...)`));
    return;
  }
  for (let i = 0; i < steps.length; i += 1) {
    const step = steps[i];
    const expectedId = `s${i + 1}`;
    const stepId = nodeText(step.children[1]);
    if (stepId !== expectedId) {
      diagnostics.push(diag(file, step.loc, `function "${fnId}" core step ${i + 1} must be "${expectedId}", got ${JSON.stringify(stepId)}`));
    }
    const stepProps = readKeywordProps(step, { start: 2 });
    const logic = nodeText(stepProps[':logic']?.value);
    if (!logic || logic.trim() === '') {
      diagnostics.push(diag(file, step.loc, `function "${fnId}" step "${stepId ?? expectedId}" must declare :logic`));
    }
  }
}

function childSection(node, sectionHead) {
  return node.children.find((child) => isList(child) && head(child) === sectionHead);
}

function listTexts(node) {
  if (!node || !isList(node)) return [];
  return node.children.map((child) => nodeText(child)).filter((text) => text != null && text !== '');
}

function diag(file, loc, message) {
  return {
    file,
    line: loc?.line ?? 1,
    column: loc?.column ?? 1,
    message,
  };
}

function runDryFixture(opts) {
  const allSurfaceForms = EXPECTED_SURFACES
    .map((surface) => `    (surface ${surface} :status "code-aligned" :code ["x"] :note "n")`)
    .join('\n');
  const allFunctionForms = EXPECTED_SURFACES
    .map((surface, index) => `    (function fn-${index + 1}
      :surface ${surface}
      :entry [in-${index + 1}]
      :core ((step s1 :logic "read input")
             (step s2 :logic "apply ${surface} contract"))
      :egress [out-${index + 1}])`)
    .join('\n');
  const goodSource = `(missiond-blueprint
  (implementation-map
${allSurfaceForms})
  (pillar-flow-map
    :schema "missiond.pillar-flow-map.v1"
    (pillar runtime
${allFunctionForms}))
  (compression-contract
    :checks ["${CHECK_COMMAND}"]))`;

  const cases = [
    {
      name: 'good fixture: every expected surface has one ordered function',
      expectOk: true,
      source: goodSource,
    },
    {
      name: 'missing function mapping fixture',
      expectOk: false,
      expectMessage: /surface "ops-infra" is not mapped/,
      source: goodSource.replace(':surface ops-infra', ':surface ops-infra-GHOST'),
    },
    {
      name: 'missing entry fixture',
      expectOk: false,
      expectMessage: /non-empty :entry/,
      source: goodSource.replace(':entry [in-1]', ':entry []'),
    },
    {
      name: 'unordered core fixture',
      expectOk: false,
      expectMessage: /core step 2 must be "s2"/,
      source: goodSource.replace('(step s2 :logic "apply mission_request contract")', '(step s9 :logic "apply mission_request contract")'),
    },
    {
      name: 'missing checker pin fixture',
      expectOk: false,
      expectMessage: /compression-contract :checks must include/,
      source: goodSource.replace(`:checks ["${CHECK_COMMAND}"]`, ':checks []'),
    },
  ];

  let failed = 0;
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-pillar-flow-'));
  try {
    for (const c of cases) {
      const file = path.join(tmp, `${c.name.replace(/\W+/g, '-')}.lisp`);
      fs.writeFileSync(file, c.source);
      const result = validatePillarFlowSource(c.source, file);
      if (result.ok !== c.expectOk) {
        failed += 1;
        console.error(`fixture FAILED: ${c.name}: expected ok=${c.expectOk}, got ok=${result.ok}`);
        for (const d of result.diagnostics) console.error(`  ${d.message}`);
        continue;
      }
      if (c.expectMessage) {
        const messages = result.diagnostics.map((d) => d.message).join(' | ');
        if (!c.expectMessage.test(messages)) {
          failed += 1;
          console.error(`fixture FAILED: ${c.name}: expected ${c.expectMessage}, got ${messages || '(none)'}`);
        }
      }
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }

  if (failed > 0) {
    console.error(`v3 pillar-flow fixtures FAILED -- ${failed}/${cases.length}`);
    process.exit(1);
  }
  if (opts.json) {
    console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
  } else {
    console.log(`v3 pillar-flow fixtures OK (${cases.length} cases)`);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
