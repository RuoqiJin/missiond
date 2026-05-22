#!/usr/bin/env node

// MissionD V3 <-> V2 convergence gate.
//
// This checker makes the "V3 is the only engineering SSOT, V2 is historical
// evidence" rule executable:
//   - Every V2 convergence item must land in a V3 pillar/function/surface.
//   - "missing" is forbidden; not-yet-implemented work must be explicit
//     `designed` instead of invisible.
//   - Every existing V3 code-aligned surface must be covered by a V2 item.
//   - Every public MCP tool definition must appear in the public surface map.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  head,
  isList,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';
import { EXPECTED_SURFACES } from './check-v3-code-isomorphism-complete.mjs';
import {
  compiledSurfaceIds,
  loadCompiledV3Contract,
} from './lib/v3_compiled_contract.mjs';
import { readBlueprintResolvedSource } from './lib/v3_blueprint_contract_source.mjs';

const BLUEPRINT_PATH = '.missiond/v3/missiond-blueprint.lisp';
const CHECK_COMMAND = 'node scripts/check-v3-v2-coverage.mjs';
const ALLOWED_STATUSES = ['missing', 'designed', 'code-aligned', 'runtime-projected'];
const FORBIDDEN_DEFERRED_RUNTIME_PHRASES = [
  'runtime projection remains',
  'future graduation',
  'later runtime-projected graduation',
  'runtime-projected graduation',
];

const usage = `Usage:
  node scripts/check-v3-v2-coverage.mjs [--json] [--dry-fixture]
    [--blueprint <path>] [--repo <path>]

Validates V3's V2 convergence and public-surface coverage:
  - (v2-convergence-map ...) exists and declares the status enum.
  - Every (v2-item ...) maps to existing V3 pillar/function/surface refs.
  - No convergence item or public tool group may use :status missing.
  - All current V3 code-aligned surfaces are covered by code-aligned or
    runtime-projected V2 items.
  - Every MCP ToolDefinition::new("mission_*", ...) under crates/missiond-mcp
    appears in exactly one public tool group.
  - compression-contract :checks pins this checker.
  - V3 notes may not hide deferred runtime projection work behind prose.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    runDryFixture(opts);
    return;
  }

  const blueprintAbs = path.resolve(opts.repo, opts.blueprint);
  const toolNames = scanMcpToolNames(path.resolve(opts.repo, 'crates/missiond-mcp/src/tools'));
  const source = readBlueprintResolvedSource(path.resolve(opts.repo), opts.blueprint);
  const compiled = loadCompiledV3Contract({
    repoRoot: path.resolve(opts.repo),
    blueprint: opts.blueprint,
    semanticIr: true,
  });
  const expectedSurfaces = compiledSurfaceIds(compiled);
  const result = validateV2CoverageSource(source, {
    file: blueprintAbs,
    repoRoot: path.resolve(opts.repo),
    toolNames,
    expectedSurfaces: expectedSurfaces.length > 0 ? expectedSurfaces : EXPECTED_SURFACES,
  });
  result.surface_source = expectedSurfaces.length > 0
    ? 'missiond-lispc emit-semantic-ir'
    : 'bootstrap-fallback';
  result.typed_surface_count = expectedSurfaces.length;
  result.diagnostics.push(...compiled.diagnostics);
  result.ok = result.ok && compiled.ok === true && expectedSurfaces.length > 0;

  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(
      `v3 v2-convergence coverage OK (${result.v2_items} V2 items, ${result.public_tools} public tools, ${result.code_aligned_surfaces} code-aligned surfaces)`,
    );
  } else {
    for (const d of result.diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.message}`);
    }
    console.error('v3 v2-convergence coverage FAILED');
  }

  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = {
    json: false,
    dryFixture: false,
    blueprint: BLUEPRINT_PATH,
    repo: process.cwd(),
  };
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
    } else if (arg === '--repo') {
      opts.repo = argv[++i] ?? fail('--repo requires a value');
    } else if (arg.startsWith('--repo=')) {
      opts.repo = arg.slice('--repo='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

function fail(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

export function validateV2CoverageSource(source, {
  file = BLUEPRINT_PATH,
  repoRoot = null,
  toolNames = [],
  expectedSurfaces = EXPECTED_SURFACES,
} = {}) {
  const diagnostics = [];
  let forms;
  try {
    forms = parseLisp(source, file);
  } catch (err) {
    return {
      ok: false,
      diagnostics: [diag(file, { line: err.line ?? 1, column: err.column ?? 1 }, err.message)],
      v2_items: 0,
      public_tools: 0,
      code_aligned_surfaces: 0,
    };
  }

  const root = forms.find((form) => isList(form) && head(form) === 'missiond-blueprint');
  if (!root) {
    diagnostics.push(diag(file, { line: 1, column: 1 }, 'missing (missiond-blueprint ...) root'));
    return finish(diagnostics);
  }
  validateNoDeferredRuntimeNotes({ source, file, diagnostics });

  const implementationMap = childSection(root, 'implementation-map');
  const flowMap = childSection(root, 'pillar-flow-map');
  const convergenceMap = childSection(root, 'v2-convergence-map');

  if (!implementationMap) diagnostics.push(diag(file, root.loc, 'missing (implementation-map ...) section'));
  if (!flowMap) diagnostics.push(diag(file, root.loc, 'missing (pillar-flow-map ...) section'));
  if (!convergenceMap) diagnostics.push(diag(file, root.loc, 'missing (v2-convergence-map ...) section'));
  if (!implementationMap || !flowMap || !convergenceMap) return finish(diagnostics);

  const surfaces = collectImplementationSurfaces(implementationMap);
  const functions = collectPillarFunctions(flowMap);
  validateStatusEnum(file, convergenceMap, diagnostics);
  validateSurfaceCodePaths(file, surfaces, repoRoot, diagnostics);

  const codeAlignedCovered = new Set();
  const v2Items = convergenceMap.children.filter((node) => isList(node) && head(node) === 'v2-item');
  if (v2Items.length === 0) {
    diagnostics.push(diag(file, convergenceMap.loc, 'v2-convergence-map must contain at least one (v2-item ...)'));
  }

  for (const item of v2Items) {
    const id = nodeText(item.children[1]) ?? '<missing>';
    const props = readKeywordProps(item, { start: 2 });
    const status = normalizeStatus(nodeText(props[':status']?.value));
    validateMappedRecord({
      file,
      node: item,
      id,
      kind: 'v2-item',
      props,
      status,
      surfaces,
      functions,
      diagnostics,
      repoRoot,
    });
    if (status === 'code-aligned' || status === 'runtime-projected') {
      for (const surface of surfaceRefs(props)) {
        if (expectedSurfaces.includes(surface)) codeAlignedCovered.add(surface);
      }
    }
  }

  for (const expected of expectedSurfaces) {
    if (!codeAlignedCovered.has(expected)) {
      diagnostics.push(diag(
        file,
        convergenceMap.loc,
        `expected code-aligned surface "${expected}" must be covered by a code-aligned/runtime-projected v2-item`,
      ));
    }
  }

  const publicMap = childSection(convergenceMap, 'public-surface-map');
  const publicToolSet = new Map();
  if (!publicMap) {
    diagnostics.push(diag(file, convergenceMap.loc, 'v2-convergence-map missing (public-surface-map ...) section'));
  } else {
    const groups = publicMap.children.filter((node) => isList(node) && head(node) === 'tool-group');
    if (groups.length === 0) {
      diagnostics.push(diag(file, publicMap.loc, 'public-surface-map must contain at least one (tool-group ...)'));
    }
    for (const group of groups) {
      const id = nodeText(group.children[1]) ?? '<missing>';
      const props = readKeywordProps(group, { start: 2 });
      const status = normalizeStatus(nodeText(props[':status']?.value));
      validateMappedRecord({
        file,
        node: group,
        id,
        kind: 'tool-group',
        props,
        status,
        surfaces,
        functions,
        diagnostics,
        repoRoot,
      });
      const tools = nodeToStringArray(props[':tools']?.value);
      if (tools.length === 0) {
        diagnostics.push(diag(file, group.loc, `tool-group "${id}" must declare non-empty :tools [...]`));
      }
      for (const tool of tools) {
        const existing = publicToolSet.get(tool);
        if (existing) {
          diagnostics.push(diag(file, group.loc, `public MCP tool "${tool}" is mapped twice: ${existing} and ${id}`));
        } else {
          publicToolSet.set(tool, id);
        }
      }
    }
  }

  const actualToolNames = [...new Set(toolNames)].sort();
  for (const tool of actualToolNames) {
    if (!publicToolSet.has(tool)) {
      diagnostics.push(diag(file, publicMap?.loc ?? convergenceMap.loc, `public MCP tool "${tool}" is not mapped in public-surface-map`));
    }
  }
  for (const tool of publicToolSet.keys()) {
    if (actualToolNames.length > 0 && !actualToolNames.includes(tool)) {
      diagnostics.push(diag(file, publicMap?.loc ?? convergenceMap.loc, `public-surface-map names unknown MCP tool "${tool}"`));
    }
  }

  const compressionContract = childSection(root, 'compression-contract');
  if (!compressionContract) {
    diagnostics.push(diag(file, root.loc, 'missing (compression-contract ...) section'));
  } else {
    const props = readKeywordProps(compressionContract, { start: 1 });
    const checks = nodeToStringArray(props[':checks']?.value);
    if (!checks.includes(CHECK_COMMAND)) {
      diagnostics.push(diag(file, compressionContract.loc, `compression-contract :checks must include "${CHECK_COMMAND}"`));
    }
  }

  return {
    ok: diagnostics.length === 0,
    diagnostics,
    v2_items: v2Items.length,
    public_tools: publicToolSet.size,
    code_aligned_surfaces: codeAlignedCovered.size,
  };
}

function validateNoDeferredRuntimeNotes({ source, file, diagnostics }) {
  const lowerSource = source.toLowerCase();
  for (const phrase of FORBIDDEN_DEFERRED_RUNTIME_PHRASES) {
    let offset = lowerSource.indexOf(phrase);
    while (offset !== -1) {
      diagnostics.push(diag(
        file,
        locForOffset(source, offset),
        `blueprint may not contain deferred runtime projection phrase "${phrase}"; encode the policy in V3 or mark the work designed explicitly`,
      ));
      offset = lowerSource.indexOf(phrase, offset + phrase.length);
    }
  }
}

function validateMappedRecord({
  file,
  node,
  id,
  kind,
  props,
  status,
  surfaces,
  functions,
  diagnostics,
  repoRoot,
}) {
  if (!status) {
    diagnostics.push(diag(file, node.loc, `${kind} "${id}" must declare :status`));
  } else if (!ALLOWED_STATUSES.includes(status)) {
    diagnostics.push(diag(file, node.loc, `${kind} "${id}" has invalid :status "${status}"`));
  } else if (status === 'missing') {
    diagnostics.push(diag(file, node.loc, `${kind} "${id}" may not remain :status missing; use designed with an explicit V3 destination`));
  }

  const source = nodeText(props[':v2-source']?.value);
  if (kind === 'v2-item' && (!source || source.trim() === '')) {
    diagnostics.push(diag(file, node.loc, `${kind} "${id}" must declare :v2-source`));
  }
  if (source) {
    validateV2SourceRef({ file, node, id, kind, source, repoRoot, diagnostics });
  }

  const pillar = nodeText(props[':v3-pillar']?.value);
  const functionRefs = functionRefsFromProps(props);
  if (!pillar) {
    diagnostics.push(diag(file, node.loc, `${kind} "${id}" must declare :v3-pillar`));
  }
  if (functionRefs.length === 0) {
    diagnostics.push(diag(file, node.loc, `${kind} "${id}" must declare :v3-function or :v3-functions [...]`));
  }
  for (const fn of functionRefs) {
    if (!functions.has(`${pillar}/${fn}`)) {
      diagnostics.push(diag(file, node.loc, `${kind} "${id}" references unknown V3 function "${pillar}/${fn}"`));
    }
  }

  const refs = surfaceRefs(props);
  if (refs.length === 0) {
    diagnostics.push(diag(file, node.loc, `${kind} "${id}" must declare :surface or :surfaces [...]`));
  }
  for (const surface of refs) {
    const surfaceInfo = surfaces.get(surface);
    if (!surfaceInfo) {
      diagnostics.push(diag(file, node.loc, `${kind} "${id}" references unknown implementation surface "${surface}"`));
      continue;
    }
    if ((status === 'code-aligned' || status === 'runtime-projected') && surfaceInfo.status !== 'code-aligned') {
      diagnostics.push(diag(
        file,
        node.loc,
        `${kind} "${id}" is ${status} but surface "${surface}" has implementation status "${surfaceInfo.status}"`,
      ));
    }
  }
}

function validateV2SourceRef({ file, node, id, kind, source, repoRoot, diagnostics }) {
  if (!repoRoot) return;
  const pathPart = source.split('::')[0]?.trim();
  if (!pathPart) return;
  const isRepoRelative = !path.isAbsolute(pathPart) && !pathPart.includes('<');
  if (!isRepoRelative) return;
  const abs = path.resolve(repoRoot, pathPart);
  if (!fs.existsSync(abs)) {
    diagnostics.push(diag(file, node.loc, `${kind} "${id}" references missing V2 source path "${pathPart}"`));
  }
}

function validateStatusEnum(file, convergenceMap, diagnostics) {
  const props = readKeywordProps(convergenceMap, { start: 1 });
  const enumValues = nodeToStringArray(props[':status-enum']?.value);
  const missing = ALLOWED_STATUSES.filter((status) => !enumValues.includes(status));
  const extra = enumValues.filter((status) => !ALLOWED_STATUSES.includes(status));
  if (missing.length > 0 || extra.length > 0) {
    diagnostics.push(diag(
      file,
      convergenceMap.loc,
      `v2-convergence-map :status-enum must be [${ALLOWED_STATUSES.join(' ')}]; missing=[${missing.join(', ')}], extra=[${extra.join(', ')}]`,
    ));
  }
}

function collectImplementationSurfaces(implementationMap) {
  const surfaces = new Map();
  for (const node of implementationMap.children.filter((child) => isList(child) && head(child) === 'surface')) {
    const id = nodeText(node.children[1]);
    if (!id) continue;
    const props = readKeywordProps(node, { start: 2 });
    surfaces.set(id, {
      status: normalizeStatus(nodeText(props[':status']?.value)),
      codePaths: nodeToStringArray(props[':code']?.value),
      node,
    });
  }
  return surfaces;
}

function validateSurfaceCodePaths(file, surfaces, repoRoot, diagnostics) {
  if (!repoRoot) return;
  for (const [surface, info] of surfaces.entries()) {
    for (const codePath of info.codePaths) {
      if (codePath.includes('<') || codePath.startsWith('node ') || codePath.startsWith('bash ')) continue;
      const abs = path.resolve(repoRoot, codePath);
      if (!fs.existsSync(abs)) {
        diagnostics.push(diag(file, info.node.loc, `implementation surface "${surface}" references missing code path "${codePath}"`));
      }
    }
  }
}

function collectPillarFunctions(flowMap) {
  const functions = new Set();
  for (const pillar of flowMap.children.filter((node) => isList(node) && head(node) === 'pillar')) {
    const pillarId = nodeText(pillar.children[1]);
    if (!pillarId) continue;
    for (const fn of pillar.children.filter((node) => isList(node) && head(node) === 'function')) {
      const fnId = nodeText(fn.children[1]);
      if (fnId) functions.add(`${pillarId}/${fnId}`);
    }
  }
  return functions;
}

function surfaceRefs(props) {
  const refs = [];
  const single = nodeText(props[':surface']?.value);
  if (single) refs.push(single);
  refs.push(...nodeToStringArray(props[':surfaces']?.value));
  return [...new Set(refs)];
}

function functionRefsFromProps(props) {
  const refs = [];
  const single = nodeText(props[':v3-function']?.value);
  if (single) refs.push(single);
  refs.push(...nodeToStringArray(props[':v3-functions']?.value));
  return [...new Set(refs)];
}

function normalizeStatus(status) {
  return status?.replace(/^:/, '') ?? null;
}

function childSection(node, sectionHead) {
  return node.children.find((child) => isList(child) && head(child) === sectionHead);
}

function diag(file, loc, message) {
  return {
    file,
    line: loc?.line ?? 1,
    column: loc?.column ?? 1,
    message,
  };
}

function locForOffset(source, offset) {
  const before = source.slice(0, offset);
  const lines = before.split('\n');
  return {
    line: lines.length,
    column: lines[lines.length - 1].length + 1,
  };
}

function finish(diagnostics) {
  return {
    ok: diagnostics.length === 0,
    diagnostics,
    v2_items: 0,
    public_tools: 0,
    code_aligned_surfaces: 0,
  };
}

export function scanMcpToolNames(root) {
  const files = listRustFiles(root);
  const out = [];
  const re = /ToolDefinition::new\(\s*"([^"]+)"/g;
  for (const file of files) {
    const source = fs.readFileSync(file, 'utf8');
    let match;
    while ((match = re.exec(source))) out.push(match[1]);
  }
  return [...new Set(out)].sort();
}

function listRustFiles(root) {
  if (!fs.existsSync(root)) return [];
  const out = [];
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const p = path.join(root, entry.name);
    if (entry.isDirectory()) {
      out.push(...listRustFiles(p));
    } else if (entry.isFile() && entry.name.endsWith('.rs')) {
      out.push(p);
    }
  }
  return out;
}

function runDryFixture(opts) {
  const allSurfaceForms = [
    ...EXPECTED_SURFACES.map((surface) => `    (surface ${surface} :status "code-aligned" :code ["x"] :note "n")`),
    '    (surface legacy-designed :status "designed" :code ["legacy.rs"] :note "planned")',
  ].join('\n');
  const allFunctionForms = [
    ...EXPECTED_SURFACES.map((surface, index) => `      (function fn-${index + 1}
        :surface ${surface}
        :entry [in-${index + 1}]
        :core ((step s1 :logic "map ${surface}"))
        :egress [out-${index + 1}])`),
    `      (function legacy-fn
        :surface legacy-designed
        :entry [mission_legacy]
        :core ((step s1 :logic "designed V3 destination"))
        :egress [legacy-result])`,
  ].join('\n');
  const allV2Items = EXPECTED_SURFACES.map((surface, index) => `    (v2-item item-${index + 1}
      :status code-aligned
      :v2-source ".missiond/v2/example.lisp :: ${surface}"
      :v3-pillar runtime
      :v3-function fn-${index + 1}
      :surface ${surface}
      :note "covered")`).join('\n');

  const goodSource = `(missiond-blueprint
  (implementation-map
${allSurfaceForms})
  (pillar-flow-map
    (pillar runtime
${allFunctionForms}))
  (v2-convergence-map
    :status-enum [missing designed code-aligned runtime-projected]
${allV2Items}
    (v2-item legacy-planned
      :status designed
      :v2-source ".missiond/v2/example.lisp :: legacy"
      :v3-pillar runtime
      :v3-function legacy-fn
      :surface legacy-designed
      :note "visible but not code-aligned")
    (public-surface-map
      (tool-group request
        :status code-aligned
        :v2-source ".missiond/v2/intent.lisp"
        :v3-pillar runtime
        :v3-function fn-1
        :surface mission_request
        :tools [mission_request])
      (tool-group legacy
        :status designed
        :v2-source ".missiond/v2/intent.lisp"
        :v3-pillar runtime
        :v3-function legacy-fn
        :surface legacy-designed
        :tools [mission_legacy])))
  (compression-contract
    :checks ["${CHECK_COMMAND}"]))`;

  const cases = [
    {
      name: 'good fixture: all expected surfaces plus public tools covered',
      expectOk: true,
      source: goodSource,
      toolNames: ['mission_request', 'mission_legacy'],
    },
    {
      name: 'missing status fixture',
      expectOk: false,
      expectMessage: /may not remain :status missing/,
      source: goodSource.replace(':status designed\n      :v2-source ".missiond/v2/example.lisp :: legacy"', ':status missing\n      :v2-source ".missiond/v2/example.lisp :: legacy"'),
      toolNames: ['mission_request', 'mission_legacy'],
    },
    {
      name: 'unmapped public tool fixture',
      expectOk: false,
      expectMessage: /public MCP tool "mission_extra" is not mapped/,
      source: goodSource,
      toolNames: ['mission_request', 'mission_legacy', 'mission_extra'],
    },
    {
      name: 'deferred runtime prose fixture',
      expectOk: false,
      expectMessage: /deferred runtime projection phrase "runtime projection remains"/,
      source: goodSource.replace(':note "visible but not code-aligned"', ':note "runtime projection remains a later runtime-projected graduation"'),
      toolNames: ['mission_request', 'mission_legacy'],
    },
    {
      name: 'unknown function fixture',
      expectOk: false,
      expectMessage: /unknown V3 function/,
      source: goodSource.replace(':v3-function legacy-fn\n      :surface legacy-designed', ':v3-function ghost-fn\n      :surface legacy-designed'),
      toolNames: ['mission_request', 'mission_legacy'],
    },
    {
      name: 'missing checker pin fixture',
      expectOk: false,
      expectMessage: /compression-contract :checks must include/,
      source: goodSource.replace(`:checks ["${CHECK_COMMAND}"]`, ':checks []'),
      toolNames: ['mission_request', 'mission_legacy'],
    },
  ];

  let failed = 0;
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-v2-coverage-'));
  try {
    for (const c of cases) {
      const file = path.join(tmp, `${c.name.replace(/\W+/g, '-')}.lisp`);
      fs.writeFileSync(file, c.source);
      const result = validateV2CoverageSource(c.source, { file, toolNames: c.toolNames });
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
    console.error(`v3 v2-convergence fixtures FAILED -- ${failed}/${cases.length}`);
    process.exit(1);
  }
  if (opts.json) {
    console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
  } else {
    console.log(`v3 v2-convergence fixtures OK (${cases.length} cases)`);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
