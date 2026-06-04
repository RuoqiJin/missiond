import fs from 'node:fs';
import crypto from 'node:crypto';
import os from 'node:os';
import path from 'node:path';

import {
  head,
  isList,
  listLispFilesRecursive,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
} from './missiond_lisp.mjs';

const SOURCE_EXTENSIONS = new Set([
  '.rs',
  '.js',
  '.mjs',
  '.cjs',
  '.ts',
  '.tsx',
  '.py',
]);

const EXCLUDED_DIRS = new Set([
  '.claude',
  '.codex',
  '.git',
  '.missiond',
  '.next',
  '.turbo',
  '.cache',
  '.vercel',
  'coverage',
  'dist',
  'build',
  'node_modules',
  'target',
  '__pycache__',
]);

const GENERATED_PATH_PARTS = [
  '/generated/',
  '/fixtures/',
  '/fixture/',
  '/__fixtures__/',
  '/testdata/',
  '/snapshots/',
];

const TEST_FILE_RE = /(^|\/)(tests?|__tests__)(\/|$)|(^|\/)tests\.rs$|(_test|_tests|\.test|\.spec)\.(rs|js|mjs|cjs|ts|tsx|py)$/;
const NAVIGATION_RISK_KINDS = new Set([
  'worker',
  'scheduler',
  'background-task',
  'mcp-tool',
  'route',
  'cli',
  'subprocess',
]);
export const COMPILED_BEHAVIOR_NAVIGATION_SCHEMA_VERSION = 'missiond.compiled-behavior-navigation.v2';
export const BEHAVIOR_UNIVERSE_SCANNER_VERSION = 'missiond.behavior-universe.scanner.v2';

export function scanObservedUniverse(root, {
  projectId = path.basename(root),
  includeTests = false,
} = {}) {
  const files = listSourceFiles(root, { includeTests });
  const observed = [];
  for (const file of files) {
    const rel = normalizeRel(root, file);
    const source = fs.readFileSync(file, 'utf8');
    if (rel.endsWith('.rs')) scanRustFile({ observed, source, rel, projectId });
    else if (/\.(js|mjs|cjs|ts|tsx)$/.test(rel)) scanJsTsFile({ observed, source, rel, projectId });
    else if (rel.endsWith('.py')) scanPythonFile({ observed, source, rel, projectId });
  }
  return observed.sort((a, b) => effectiveObservedId(a).localeCompare(effectiveObservedId(b)));
}

export function loadDeclaredBehaviorUniverse(root, {
  projectId = path.basename(root),
  missiondV3 = false,
} = {}) {
  const lispFiles = behaviorLispFiles(root, { missiondV3 });
  const universes = [];
  const diagnostics = [];
  for (const file of lispFiles) {
    let forms;
    try {
      forms = parseLisp(fs.readFileSync(file, 'utf8'), file);
    } catch (err) {
      diagnostics.push({
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        code: 'BEHAVIOR_LISP_PARSE_ERROR',
        message: err.message,
      });
      continue;
    }
    for (const form of collectForms(forms, 'behavior-universe')) {
      universes.push(parseBehaviorUniverse(root, file, form));
    }
  }
  const navigation = loadCompiledBehaviorNavigation(root, projectId, { missiondV3 });

  const behaviors = universes.flatMap((u) => u.behaviors);
  const effects = universes.flatMap((u) => u.effects);
  const tombstones = universes.flatMap((u) => u.tombstones);
  const scannerProfiles = universes.flatMap((u) => u.scannerProfiles);
  const anchors = [
    ...behaviors.flatMap((behavior) => behavior.anchors),
    ...navigation.anchors,
  ];
  const triggers = behaviors.flatMap((behavior) => behavior.triggers);

  return {
    projectId,
    files: lispFiles,
    universes,
    behaviors,
    effects,
    tombstones,
    scannerProfiles,
    anchors,
    triggers,
    diagnostics,
    projectionDiagnostics: navigation.diagnostics,
    navigationArtifact: navigation.artifact,
  };
}

export function validateBehaviorClosure(root, {
  projectId = path.basename(root),
  missiondV3 = false,
  includeTests = false,
  navigationLevel = 'risk',
} = {}) {
  const declared = loadDeclaredBehaviorUniverse(root, { projectId, missiondV3 });
  const observed = scanObservedUniverse(root, { projectId, includeTests });
  const diagnostics = [...declared.diagnostics];
  const projectionDiagnostics = [...(declared.projectionDiagnostics ?? [])];

  if (declared.universes.length === 0) {
    diagnostics.push({
      file: path.join(root, '.missiond'),
      line: 1,
      column: 1,
      code: 'BEHAVIOR_UNIVERSE_MISSING',
      message: `${projectId} has active code but no (behavior-universe ...) SSOT`,
    });
  }

  const behaviorPatterns = declared.behaviors.flatMap((behavior) => behavior.observed);
  const tombstonePatterns = declared.tombstones.map((t) => t.observedId).filter(Boolean);
  const effectsById = new Map(declared.effects.map((effect) => [effect.id, effect]));

  for (const behavior of declared.behaviors) {
    if (!behavior.id) {
      diagnostics.push(diag(behavior, 'BEHAVIOR_ID_MISSING', 'behavior missing :id'));
    }
    if (!behavior.kind) {
      diagnostics.push(diag(behavior, 'BEHAVIOR_KIND_MISSING', `behavior ${behavior.id ?? '<missing>'} missing :kind`));
    }
    if (!behavior.owner) {
      diagnostics.push(diag(behavior, 'BEHAVIOR_OWNER_MISSING', `behavior ${behavior.id ?? '<missing>'} missing :owner`));
    }
    if (behavior.observed.length === 0) {
      diagnostics.push(diag(behavior, 'BEHAVIOR_OBSERVED_MISSING', `behavior ${behavior.id ?? '<missing>'} missing :observed`));
    }
    if (behavior.code.length === 0) {
      diagnostics.push(diag(behavior, 'BEHAVIOR_CODE_MISSING', `behavior ${behavior.id ?? '<missing>'} missing :code`));
    }
    for (const effectId of behavior.effects) {
      if (!effectsById.has(effectId)) {
        diagnostics.push(diag(behavior, 'BEHAVIOR_EFFECT_UNKNOWN', `behavior ${behavior.id} references unknown effect ${effectId}`));
      }
    }
    for (const observedId of behavior.observed) {
      if (observedId.includes('*')) continue;
      if (observed.some((item) => observedMatchesPattern(item, observedId))) continue;
      diagnostics.push(diag(behavior, 'DECLARED_OBSERVED_ID_MISSING', `behavior ${behavior.id} declares ${observedId}, but scanner did not observe it in active code`));
    }
  }

  for (const effect of declared.effects) {
    if (!effect.id) diagnostics.push(diag(effect, 'EFFECT_ID_MISSING', 'effect missing :id'));
    if (!effect.feature) diagnostics.push(diag(effect, 'EFFECT_FEATURE_MISSING', `effect ${effect.id ?? '<missing>'} missing :feature`));
    if (!effect.operation) diagnostics.push(diag(effect, 'EFFECT_OPERATION_MISSING', `effect ${effect.id ?? '<missing>'} missing :operation`));
    if (!effect.pathPattern) diagnostics.push(diag(effect, 'EFFECT_PATH_PATTERN_MISSING', `effect ${effect.id ?? '<missing>'} missing :path-pattern`));
    if (effect.scope === 'external-home' && !effect.audit) {
      diagnostics.push(diag(effect, 'EFFECT_AUDIT_MISSING', `external-home effect ${effect.id ?? '<missing>'} missing :audit`));
    }
  }

  for (const item of observed) {
    if (item.kind !== 'effect') continue;
    if (matchesAnyObserved(item, tombstonePatterns)) continue;
    if (declared.effects.some((effect) => declaredEffectCoversObserved(effect, item))) continue;
    diagnostics.push({
      file: path.join(root, item.file),
      line: item.line,
      column: 1,
      code: 'OBSERVED_EFFECT_UNDECLARED',
      message: `${item.id} (${item.operation}, ${item.scope}) is filesystem behavior but has no matching (effect ...) contract`,
    });
  }

  for (const item of observed) {
    if (matchesAnyObserved(item, behaviorPatterns) || matchesAnyObserved(item, tombstonePatterns)) continue;
    diagnostics.push({
      file: path.join(root, item.file),
      line: item.line,
      column: 1,
      code: 'OBSERVED_BEHAVIOR_UNCLAIMED',
      message: `${item.id} (${item.kind}) is active code behavior but is not claimed by behavior-universe`,
    });
  }

  for (const item of observed) {
    if (item.kind !== 'effect' || item.scope !== 'external-home') continue;
    if (item.guard === true) continue;
    diagnostics.push({
      file: path.join(root, item.file),
      line: item.line,
      column: 1,
      code: 'EXTERNAL_EFFECT_GUARD_BYPASS',
      message: `${item.id} writes external/user-home state without context::effects guard`,
    });
  }

  if (navigationLevel !== 'off') {
    validateNavigationClosure({
      root,
      observed,
      declared,
      effectsById,
      behaviorPatterns,
      tombstonePatterns,
      navigationLevel,
      diagnostics,
      projectionDiagnostics,
    });
  }

  return {
    ok: diagnostics.length === 0,
    projection_ok: projectionDiagnostics.length === 0,
    projectId,
    observed,
    declared,
    diagnostics,
    projectionDiagnostics,
  };
}

function missiondRuntimeDir() {
  const runtimeDir = process.env.MISSIOND_RUNTIME_DIR
    || path.join(os.homedir(), '.missiond/runtime/missiond');
  return runtimeDir;
}

function missiondRuntimeCompiledDir() {
  return path.join(missiondRuntimeDir(), 'compiled');
}

export function behaviorNavigationRuntimeTarget(projectId) {
  if (projectId === 'missiond') {
    return path.join(missiondRuntimeCompiledDir(), 'compiled-behavior-navigation.json');
  }
  return path.join(missiondRuntimeDir(), 'projects', projectId, 'compiled', 'compiled-behavior-navigation.json');
}

function behaviorNavigationCandidates(root, projectId, { missiondV3 = false } = {}) {
  const candidates = [behaviorNavigationRuntimeTarget(projectId)];
  if (projectId !== 'missiond') {
    candidates.push(path.join(root, '.missiond/runtime/compiled/compiled-behavior-navigation.json'));
  }
  if (missiondV3 || projectId === 'missiond') {
    candidates.push(path.join(root, '.missiond/v3/runtime/compiled/compiled-behavior-navigation.json'));
  }
  return uniqueStrings(candidates);
}

function loadCompiledBehaviorNavigation(root, projectId, { missiondV3 = false } = {}) {
  const candidates = behaviorNavigationCandidates(root, projectId, { missiondV3 });
  const file = candidates.find((candidate) => fs.existsSync(candidate)) ?? candidates[0];
  if (!fs.existsSync(file)) {
    return {
      anchors: [],
      artifact: {
        file,
        schema_version: null,
        loaded: false,
        missing: true,
        candidates,
      },
      diagnostics: [],
    };
  }
  try {
    const compiled = JSON.parse(fs.readFileSync(file, 'utf8'));
    const diagnostics = [];
    const schema = compiled?.schema_version;
    if (schema !== COMPILED_BEHAVIOR_NAVIGATION_SCHEMA_VERSION && schema !== 'missiond.compiled-behavior-navigation.v1') {
      diagnostics.push({
        file,
        line: 1,
        column: 1,
        code: 'BEHAVIOR_NAVIGATION_SCHEMA_MISMATCH',
        message: `unsupported behavior navigation schema ${JSON.stringify(compiled?.schema_version)}`,
      });
    }
    if (Array.isArray(compiled?.diagnostics) && compiled.diagnostics.length > 0) {
      diagnostics.push(...compiled.diagnostics.map((diagnostic) => ({
        file,
        line: diagnostic?.line ?? 1,
        column: diagnostic?.column ?? 1,
        code: diagnostic?.code ?? 'BEHAVIOR_NAVIGATION_DIAGNOSTIC',
        message: diagnostic?.message ?? JSON.stringify(diagnostic),
      })));
    }
    const artifact = {
      file,
      schema_version: schema ?? null,
      loaded: true,
      missing: false,
      candidates,
      source_hash: compiled?.source_hash ?? null,
      scanner_version: compiled?.scanner_version ?? compiled?.payload?.scanner_version ?? null,
      project_id: compiled?.project_id ?? compiled?.payload?.projectId ?? compiled?.payload?.project_id ?? null,
    };
    if (schema === COMPILED_BEHAVIOR_NAVIGATION_SCHEMA_VERSION) {
      return {
        anchors: arrayOrEmpty(compiled?.anchors ?? compiled?.payload?.anchors)
          .map((anchor, index) => compiledAnchorToDeclaredAnchor({ root, file, anchor, index })),
        artifact,
        diagnostics,
      };
    }
    const forms = typeof compiled?.payload?.forms === 'string' ? compiled.payload.forms : '';
    if (forms.trim()) {
      const source = `(behavior-universe ${projectId}
  :schema "missiond.behavior-universe.navigation.v1"
  :project ${projectId}
  :status generated-runtime-navigation
  :owner navigation-gate
${forms}
)`;
      const parsed = parseLisp(source, file);
      const universes = collectForms(parsed, 'behavior-universe')
        .map((form) => parseBehaviorUniverse(root, file, form));
      return {
        anchors: universes.flatMap((universe) => universe.behaviors.flatMap((behavior) => behavior.anchors)),
        artifact,
        diagnostics,
      };
    }
    diagnostics.push({
      file,
      line: 1,
      column: 1,
      code: 'BEHAVIOR_NAVIGATION_FORMS_MISSING',
      message: 'legacy compiled behavior navigation artifact missing payload.forms',
    });
    return { anchors: [], artifact, diagnostics };
  } catch (err) {
    return {
      anchors: [],
      artifact: {
        file,
        schema_version: null,
        loaded: false,
        missing: false,
        candidates,
      },
      diagnostics: [{
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        code: 'BEHAVIOR_NAVIGATION_ARTIFACT_ERROR',
        message: err.message,
      }],
    };
  }
}

function behaviorLispFiles(root, { missiondV3 }) {
  const files = new Set();
  const missiondDir = path.join(root, '.missiond');
  if (fs.existsSync(missiondDir)) {
    for (const file of listLispFilesRecursive(missiondDir)) {
      const rel = normalizeRel(root, file);
      if (rel.includes('/runtime/') || rel.includes('/tasks/') || rel.includes('/research/')) continue;
      if (path.basename(file) === 'behavior-universe.lisp') files.add(file);
    }
  }
  if (missiondV3) {
    for (const rel of [
      '.missiond/v3/missiond-blueprint.lisp',
      '.missiond/v3/shards/universe/behavior-closure.lisp',
    ]) {
      const file = path.join(root, rel);
      if (fs.existsSync(file)) files.add(file);
    }
  }
  return [...files].sort();
}

function parseBehaviorUniverse(root, file, form) {
  const props = readKeywordProps(form, { start: 2 });
  return {
    file,
    project: textProp(props, ':project') ?? nodeText(form.children[1]) ?? null,
    behaviors: collectForms([form], 'behavior').map((node) => parseBehavior(root, file, node)),
    effects: collectForms([form], 'effect').map((node) => parseEffect(root, file, node)),
    tombstones: collectForms([form], 'tombstone').map((node) => parseTombstone(root, file, node)),
    scannerProfiles: collectForms([form], 'scanner-profile').map((node) => parseScannerProfile(root, file, node)),
  };
}

function parseBehavior(root, file, node) {
  const props = readKeywordProps(node, { start: 1 });
  const id = textProp(props, ':id');
  return {
    file,
    relFile: normalizeRel(root, file),
    line: node.loc?.line ?? 1,
    id,
    kind: textProp(props, ':kind'),
    owner: textProp(props, ':owner'),
    observed: arrayProp(props, ':observed'),
    code: arrayProp(props, ':code'),
    effects: arrayProp(props, ':effects'),
    anchors: collectForms([node], 'anchor').map((anchor) => parseAnchor(root, file, anchor, id)),
    triggers: collectForms([node], 'trigger').map((trigger) => parseTrigger(root, file, trigger, id)),
  };
}

function parseAnchor(root, file, node, behaviorId) {
  const props = readKeywordProps(node, { start: 1 });
  return {
    file,
    relFile: normalizeRel(root, file),
    line: node.loc?.line ?? 1,
    behaviorId,
    role: textProp(props, ':role'),
    observed: textProp(props, ':observed'),
    semanticId: textProp(props, ':semantic-id'),
    legacyObservedId: textProp(props, ':legacy-observed-id'),
    targetFile: textProp(props, ':file'),
    symbol: textProp(props, ':symbol'),
    effect: textProp(props, ':effect'),
    source: 'lisp',
  };
}

function parseTrigger(root, file, node, behaviorId) {
  const props = readKeywordProps(node, { start: 1 });
  return {
    file,
    relFile: normalizeRel(root, file),
    line: node.loc?.line ?? 1,
    behaviorId,
    fromFile: textProp(props, ':from-file'),
    fromSymbol: textProp(props, ':from-symbol'),
    calls: textProp(props, ':calls'),
  };
}

function parseEffect(root, file, node) {
  const props = readKeywordProps(node, { start: 1 });
  return {
    file,
    relFile: normalizeRel(root, file),
    line: node.loc?.line ?? 1,
    id: textProp(props, ':id'),
    feature: textProp(props, ':feature'),
    kind: textProp(props, ':kind'),
    operation: textProp(props, ':operation'),
    pathPattern: textProp(props, ':path-pattern'),
    scope: textProp(props, ':scope'),
    default: textProp(props, ':default'),
    killSwitch: textProp(props, ':kill-switch'),
    audit: textProp(props, ':audit'),
  };
}

function parseTombstone(root, file, node) {
  const props = readKeywordProps(node, { start: 1 });
  return {
    file,
    relFile: normalizeRel(root, file),
    line: node.loc?.line ?? 1,
    observedId: textProp(props, ':observed-id'),
    reason: textProp(props, ':reason'),
  };
}

function parseScannerProfile(root, file, node) {
  const props = readKeywordProps(node, { start: 1 });
  return {
    file,
    relFile: normalizeRel(root, file),
    line: node.loc?.line ?? 1,
    id: textProp(props, ':id') ?? nodeText(node.children[1]) ?? null,
    language: textProp(props, ':language'),
    mode: textProp(props, ':mode'),
    coverage: arrayProp(props, ':coverage'),
    manualContracts: arrayProp(props, ':manual-contracts'),
    rule: textProp(props, ':rule'),
  };
}

function scanRustFile({ observed, source, rel, projectId }) {
  const lines = source.split(/\r?\n/);
  const effectContexts = rustEffectContextMap(source);
  let cfgTestPending = false;
  let cfgTestBraceDepth = 0;
  lines.forEach((line, idx) => {
    if (cfgTestBraceDepth > 0) {
      cfgTestBraceDepth = Math.max(0, cfgTestBraceDepth + braceDelta(line));
      return;
    }
    if (cfgTestPending) {
      if (/^\s*$/.test(line) || /^\s*#/.test(line)) return;
      const delta = braceDelta(line);
      if (line.includes('{')) {
        cfgTestBraceDepth = Math.max(0, delta);
      } else {
        cfgTestPending = false;
      }
      if (cfgTestBraceDepth === 0) cfgTestPending = false;
      return;
    }
    if (/^\s*#\[cfg\(test\)\]/.test(line)) {
      cfgTestPending = true;
      return;
    }
    if (/^\s*\/\//.test(line)) return;
    const lineNo = idx + 1;
    const context = nearby(lines, idx);
    const symbol = rustSymbolAt(lines, idx);
    const worker = line.match(/\bimpl\s+(?:super::)?BackgroundWorker\s+for\s+([A-Za-z0-9_]+)/);
    if (worker) {
      pushObserved(observed, 'worker', `worker:${slug(worker[1])}`, rel, lineNo, 'rust-background-worker', projectId, {
        role: 'worker',
        symbol: worker[1],
      });
    }
    if (/\btokio::time::interval\b|\bstd::thread::spawn\b/.test(line)) {
      pushObserved(observed, 'scheduler', `scheduler:${rel}:${lineNo}`, rel, lineNo, 'rust-scheduler', projectId, {
        role: 'scheduler',
        symbol,
      });
    }
    if (/\btokio::spawn\b/.test(line)) {
      pushObserved(observed, 'background-task', `background-task:${rel}:${lineNo}`, rel, lineNo, 'rust-tokio-spawn', projectId, {
        role: 'scheduler',
        symbol,
      });
    }
    const tool = line.includes('ToolDefinition::new(')
      ? context.match(/ToolDefinition::new\(\s*"([^"]+)"/)
      : null;
    if (tool) {
      pushObserved(observed, 'mcp-tool', `mcp-tool:${tool[1]}`, rel, lineNo, 'rust-mcp-tool-definition', projectId, {
        role: 'tool',
        symbol: tool[1],
      });
    }
    if (/\b(route|Router::new|\.route)\s*\(/.test(line)) {
      pushObserved(observed, 'route', `route:${rel}:${lineNo}`, rel, lineNo, 'rust-route', projectId, {
        role: 'route',
        symbol,
      });
    }
    if (/\b(sqlx::query|query_as!?\(|INSERT\s+INTO|UPDATE\s+|DELETE\s+FROM)\b/i.test(line)) {
      pushObserved(observed, 'db-write', `db-write:${rel}:${lineNo}`, rel, lineNo, 'rust-db-mutation-or-query', projectId);
    }
    if (/\b(Command::new|std::process::Command|\.spawn\(\))\b/.test(line) && !line.includes('tokio::spawn')) {
      pushObserved(observed, 'subprocess', `subprocess:${rel}:${lineNo}`, rel, lineNo, 'rust-subprocess', projectId, {
        role: 'subprocess',
        symbol,
      });
    }
    if (/\b(reqwest::|ureq::|hyper::|\.post\(|\.get\(|\.send\(\))\b/.test(line)) {
      pushObserved(observed, 'network', `network:${rel}:${lineNo}`, rel, lineNo, 'rust-network', projectId);
    }
    if (/Sonnet|Gemini|Codex|Minimax|OpenAI|Anthropic|LLM|llm/i.test(rel) && /\b(send|chat|complete|generate|request|spawn)\b/i.test(line)) {
      pushObserved(observed, 'model-call', `model-call:${rel}:${lineNo}`, rel, lineNo, 'rust-model-call', projectId);
    }
    const fsOp = rustFsOperation(line);
    if (fsOp) {
      const scope = externalHomeContext(context) ? 'external-home' : 'repo-or-runtime';
      const effectHint = effectHintForLine(context, effectContexts);
      pushObserved(observed, 'effect', `effect:fs-${fsOp}:${rel}:${lineNo}`, rel, lineNo, `rust-fs-${fsOp}`, projectId, {
        operation: fsOp,
        scope,
        guard: /\bcontext::effects\b|\beffects::(write_text|atomic_write_text|append_text|remove_file)\b/.test(context),
        role: 'effect-site',
        symbol,
        effectHint,
      });
    }
  });
}

function braceDelta(line) {
  const withoutLineComment = line.replace(/\/\/.*$/, '');
  let delta = 0;
  for (const char of withoutLineComment) {
    if (char === '{') delta += 1;
    else if (char === '}') delta -= 1;
  }
  return delta;
}

function scanJsTsFile({ observed, source, rel, projectId }) {
  const lines = source.split(/\r?\n/);
  lines.forEach((line, idx) => {
    const lineNo = idx + 1;
    const context = nearby(lines, idx);
    const symbol = jsTsSymbolAt(lines, idx);
    if (fixtureContext(context)) return;
    if (detectorDefinitionContext(rel, line)) return;
    if (/export\s+(async\s+)?function\s+(GET|POST|PUT|PATCH|DELETE)\b|router\.(get|post|put|patch|delete)\s*\(/.test(line)) {
      pushObserved(observed, 'route', `route:${rel}:${lineNo}`, rel, lineNo, 'js-route', projectId, {
        role: 'route',
        symbol,
      });
    }
    if (/\b(setInterval|setTimeout|cron\.schedule|node-cron|scheduleJob)\b/.test(line)) {
      pushObserved(observed, 'scheduler', `scheduler:${rel}:${lineNo}`, rel, lineNo, 'js-scheduler', projectId, {
        role: 'scheduler',
        symbol,
      });
    }
    if (/\b(fs\.writeFileSync|fs\.writeFile|writeFileSync|writeFile|appendFileSync|appendFile|rmSync|unlinkSync|renameSync|rename)\b/.test(line)) {
      const op = /append/.test(line) ? 'append' : /rm|unlink/.test(line) ? 'delete' : /rename/.test(line) ? 'rename' : 'write';
      pushObserved(observed, 'effect', `effect:fs-${op}:${rel}:${lineNo}`, rel, lineNo, `js-fs-${op}`, projectId, {
        operation: op,
        scope: externalHomeContext(context) ? 'external-home' : 'repo-or-runtime',
        guard: false,
        role: 'effect-site',
        symbol,
      });
    }
    if (/\b(child_process|spawnSync|spawn\(|execFile)\b/.test(line) || /(?<![\w.])exec\(/.test(line)) {
      pushObserved(observed, 'subprocess', `subprocess:${rel}:${lineNo}`, rel, lineNo, 'js-subprocess', projectId, {
        role: 'subprocess',
        symbol,
      });
    }
    if (/\b(fetch\(|axios\.|got\.|undici)\b/.test(line)) {
      pushObserved(observed, 'network', `network:${rel}:${lineNo}`, rel, lineNo, 'js-network', projectId);
    }
  });
}

function scanPythonFile({ observed, source, rel, projectId }) {
  const lines = source.split(/\r?\n/);
  lines.forEach((line, idx) => {
    const lineNo = idx + 1;
    const context = nearby(lines, idx);
    const symbol = pythonSymbolAt(lines, idx);
    if (fixtureContext(context)) return;
    if (/\b(argparse\.ArgumentParser|click\.command|typer\.Typer)\b/.test(line)) {
      pushObserved(observed, 'cli', `cli:${rel}:${lineNo}`, rel, lineNo, 'python-cli', projectId, {
        role: 'entry',
        symbol,
      });
    }
    if (/\b(schedule\.every|APScheduler|BackgroundScheduler|asyncio\.create_task)\b/.test(line)) {
      pushObserved(observed, 'scheduler', `scheduler:${rel}:${lineNo}`, rel, lineNo, 'python-scheduler', projectId, {
        role: 'scheduler',
        symbol,
      });
    }
    if (/\b(open\(|Path\(.*\)\.write_text|write_bytes|os\.remove|os\.rename)\b/.test(line)) {
      const op = /remove|unlink/.test(line) ? 'delete' : /rename/.test(line) ? 'rename' : 'write';
      pushObserved(observed, 'effect', `effect:fs-${op}:${rel}:${lineNo}`, rel, lineNo, `python-fs-${op}`, projectId, {
        operation: op,
        scope: externalHomeContext(context) ? 'external-home' : 'repo-or-runtime',
        guard: false,
        role: 'effect-site',
        symbol,
      });
    }
    if (/\bsubprocess\.(run|Popen|call|check_call|check_output)\b/.test(line)) {
      pushObserved(observed, 'subprocess', `subprocess:${rel}:${lineNo}`, rel, lineNo, 'python-subprocess', projectId, {
        role: 'subprocess',
        symbol,
      });
    }
    if (/\b(requests\.|httpx\.|aiohttp\.)/.test(line)) {
      pushObserved(observed, 'network', `network:${rel}:${lineNo}`, rel, lineNo, 'python-network', projectId);
    }
  });
}

function rustEffectContextMap(source) {
  const out = new Map();
  const re = /const\s+([A-Z0-9_]+)\s*:\s*EffectContext\s*=\s*EffectContext::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,?\s*\)/g;
  let match;
  while ((match = re.exec(source)) != null) {
    out.set(match[1], { featureId: match[2], effectId: match[3] });
  }
  return out;
}

function effectHintForLine(line, effectContexts) {
  for (const [name, ctx] of effectContexts.entries()) {
    if (line.includes(name)) return ctx.effectId;
  }
  return null;
}

function rustSymbolAt(lines, idx) {
  for (let i = idx; i >= 0; i -= 1) {
    const line = lines[i];
    const fn = line.match(/\b(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)/);
    if (fn) return fn[1];
    const impl = line.match(/\bimpl(?:\s*<[^>]+>)?\s+([A-Za-z0-9_]+)/);
    if (impl) return impl[1];
  }
  return null;
}

function jsTsSymbolAt(lines, idx) {
  const sameLine = lines[idx];
  const exported = sameLine.match(/export\s+(?:async\s+)?function\s+([A-Za-z0-9_]+)/);
  if (exported) return exported[1];
  const route = sameLine.match(/\brouter\.(get|post|put|patch|delete)\s*\(/);
  if (route) return `router.${route[1]}`;
  const named = sameLine.match(/\b(?:async\s+)?function\s+([A-Za-z0-9_]+)/);
  if (named) return named[1];
  for (let i = idx; i >= 0; i -= 1) {
    const line = lines[i];
    const fn = line.match(/\b(?:export\s+)?(?:async\s+)?function\s+([A-Za-z0-9_]+)/);
    if (fn) return fn[1];
    const arrow = line.match(/\b(?:const|let|var)\s+([A-Za-z0-9_]+)\s*=\s*(?:async\s*)?\(/);
    if (arrow) return arrow[1];
  }
  return null;
}

function pythonSymbolAt(lines, idx) {
  for (let i = idx; i >= 0; i -= 1) {
    const line = lines[i];
    const fn = line.match(/^\s*(?:async\s+)?def\s+([A-Za-z0-9_]+)/);
    if (fn) return fn[1];
  }
  return null;
}

function rustFsOperation(line) {
  if (/\b(append_text|OpenOptions::new)\b/.test(line)) return 'append';
  if (/\b(tokio::fs::remove_file|std::fs::remove_file|fs::remove_file)\b/.test(line)) return 'delete';
  if (/\b(tokio::fs::rename|std::fs::rename|fs::rename)\b/.test(line)) return 'rename';
  if (/\b(std::fs::write|tokio::fs::write|fs::write|File::create|atomic_write_text|write_text)\b/.test(line)) return 'write';
  return null;
}

function pushObserved(observed, kind, id, file, line, detector, projectId, extra = {}) {
  const legacyId = normalizeObservedId(id);
  const semantic = semanticObservedIdentity({
    kind,
    file,
    line,
    role: extra.role,
    symbol: extra.symbol,
    effectHint: extra.effectHint,
    operation: extra.operation,
    legacyId,
  });
  observed.push({
    projectId,
    id: legacyId,
    legacy_id: legacyId,
    semantic_id: semantic.id,
    stability: semantic.stability,
    kind,
    file,
    line,
    detector,
    ...extra,
  });
}

function semanticObservedIdentity({ kind, file, line, role, symbol, effectHint, operation, legacyId }) {
  if (!symbol) {
    return {
      id: legacyId,
      stability: 'line-bound',
    };
  }
  const effectiveRole = role ?? roleForObservedKind(kind);
  const effectOrRole = effectHint ?? operation ?? effectiveRole ?? kind;
  return {
    id: normalizeObservedId(`${kind}:${file}#${symbol}:${effectOrRole}`),
    stability: 'symbol',
  };
}

function listSourceFiles(root, { includeTests }) {
  const out = [];
  walk(root, out, { includeTests });
  return out.sort();
}

function walk(dir, out, { includeTests }) {
  if (!fs.existsSync(dir)) return;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (EXCLUDED_DIRS.has(entry.name)) continue;
      walk(path.join(dir, entry.name), out, { includeTests });
      continue;
    }
    if (!entry.isFile()) continue;
    const file = path.join(dir, entry.name);
    const rel = file.replace(/\\/g, '/');
    if (!SOURCE_EXTENSIONS.has(path.extname(entry.name))) continue;
    if (!includeTests && TEST_FILE_RE.test(rel)) continue;
    if (GENERATED_PATH_PARTS.some((part) => rel.includes(part))) continue;
    out.push(file);
  }
}

function collectForms(forms, name) {
  const out = [];
  for (const form of forms) collectFormsInto(form, name, out);
  return out;
}

function collectFormsInto(node, name, out) {
  if (isList(node) && head(node) === name) out.push(node);
  if (!isList(node)) return;
  for (const child of node.children) collectFormsInto(child, name, out);
}

function textProp(props, key) {
  return nodeText(props[key]?.value);
}

function arrayProp(props, key) {
  return nodeToStringArray(props[key]?.value);
}

function normalizeRel(root, file) {
  return path.relative(root, file).replace(/\\/g, '/');
}

function nearby(lines, idx) {
  const start = Math.max(0, idx - 4);
  const end = Math.min(lines.length, idx + 5);
  return lines.slice(start, end).join('\n');
}

function externalHomeContext(text) {
  return /\bdirs::home_dir\b|\bhome_dir\(|\bHOME\b|~\/|\$HOME|\/Users\//.test(text);
}

function fixtureContext(text) {
  return /\bfixture\b|\bdryFixture\b|\bdry-fixture\b|\brunFixtures\b|\bselfTest\b/i.test(text);
}

function detectorDefinitionContext(rel, line) {
  return rel === 'scripts/lib/behavior_universe.mjs' && /\.test\(line\)/.test(line);
}

function validateNavigationClosure({
  root,
  observed,
  declared,
  effectsById,
  behaviorPatterns,
  tombstonePatterns,
  navigationLevel,
  diagnostics,
  projectionDiagnostics,
}) {
  const validAnchorsByObservedId = new Map();
  const riskItems = observed.filter((item) => (
    !matchesAnyObserved(item, tombstonePatterns)
    && isNavigationRisk(item, navigationLevel)
  ));
  if (riskItems.length > 0 && declared.navigationArtifact?.missing) {
    projectionDiagnostics.push({
      file: declared.navigationArtifact.file,
      line: 1,
      column: 1,
      code: 'BEHAVIOR_NAVIGATION_ARTIFACT_MISSING',
      message: `${declared.projectId} has ${riskItems.length} navigation-risk behavior(s) but no compiled navigation artifact; run node scripts/propose-behavior-navigation.mjs --project ${declared.projectId} --write`,
    });
  }
  if (declared.navigationArtifact?.loaded && declared.navigationArtifact?.source_hash) {
    const expectedHash = behaviorNavigationSourceHash({
      projectId: declared.projectId,
      root,
      sourceUnits: behaviorNavigationSourceUnits(riskItems),
    });
    if (expectedHash !== declared.navigationArtifact.source_hash) {
      projectionDiagnostics.push({
        file: declared.navigationArtifact.file,
        line: 1,
        column: 1,
        code: 'BEHAVIOR_NAVIGATION_ARTIFACT_STALE',
        message: `${declared.projectId} compiled navigation artifact is stale; run node scripts/propose-behavior-navigation.mjs --project ${declared.projectId} --write`,
      });
    }
  }

  for (const anchor of declared.anchors) {
    let anchorValid = true;
    if (!anchor.role || !anchor.observed || !anchor.targetFile) {
      projectionDiagnostics.push(diag(
        anchor,
        'NAVIGATION_ANCHOR_MISSING',
        `behavior ${anchor.behaviorId ?? '<missing>'} has an anchor missing :role, :observed, or :file`,
      ));
      anchorValid = false;
    }

    if (anchor.targetFile && !fileExists(root, anchor.targetFile)) {
      projectionDiagnostics.push(diag(
        anchor,
        'NAVIGATION_ANCHOR_FILE_MISSING',
        `anchor for ${anchor.observed ?? '<missing>'} points at missing file ${anchor.targetFile}`,
      ));
      anchorValid = false;
    }

    if (anchor.effect && !effectsById.has(anchor.effect)) {
      projectionDiagnostics.push(diag(
        anchor,
        'NAVIGATION_EFFECT_CONTRACT_MISSING',
        `anchor for ${anchor.observed ?? '<missing>'} references unknown effect ${anchor.effect}`,
      ));
      anchorValid = false;
    }

    const matchedObserved = anchor.observed
      ? observed.filter((item) => anchorObservedMatchesItem(anchor, item))
      : [];
    if (anchor.observed && matchedObserved.length === 0) {
      projectionDiagnostics.push(diag(
        anchor,
        'NAVIGATION_ANCHOR_STALE',
        `anchor observes ${anchor.observed}, but scanner did not observe a matching active behavior`,
      ));
      anchorValid = false;
    }

    const matchingObserved = matchedObserved.filter((item) => anchorMatchesObserved(root, anchor, item));
    if (matchedObserved.length > 0 && matchingObserved.length === 0) {
      projectionDiagnostics.push(diag(
        anchor,
        'NAVIGATION_ANCHOR_STALE',
        `anchor for ${anchor.observed} no longer matches any observed behavior after role/symbol/effect checks`,
      ));
    }

    for (const item of matchingObserved) {
      if (anchorValid) {
        for (const key of observedIdentityKeys(item)) {
          const anchors = validAnchorsByObservedId.get(key) ?? [];
          anchors.push(anchor);
          validAnchorsByObservedId.set(key, anchors);
        }
      }
    }
  }

  for (const trigger of declared.triggers) {
    if (trigger.fromFile && !fileExists(root, trigger.fromFile)) {
      projectionDiagnostics.push(diag(
        trigger,
        'NAVIGATION_ANCHOR_FILE_MISSING',
        `trigger for behavior ${trigger.behaviorId ?? '<missing>'} points at missing file ${trigger.fromFile}`,
      ));
    }
  }

  for (const item of riskItems) {
    const anchors = anchorsForObservedItem(validAnchorsByObservedId, item);
    if (anchors.length > 0) continue;

    const claimedPatterns = behaviorPatterns.filter((pattern) => observedMatchesPattern(item, pattern));
    if (claimedPatterns.some((pattern) => pattern.includes('*'))) {
      projectionDiagnostics.push({
        file: path.join(root, item.file),
        line: item.line,
        column: 1,
        code: 'NAVIGATION_CRITICAL_WILDCARD_ONLY',
        message: `${item.id} (${item.kind}) is high-risk behavior claimed by wildcard without a matching navigation anchor`,
      });
    }
    projectionDiagnostics.push({
      file: path.join(root, item.file),
      line: item.line,
      column: 1,
      code: 'NAVIGATION_ANCHOR_MISSING',
      message: `${item.id} (${item.kind}) is high-risk behavior but has no matching (anchor ...) in behavior-universe`,
    });
  }

  for (const effect of declared.effects) {
    if (effect.scope !== 'external-home') continue;
    const hasReferencedAnchor = declared.anchors.some((anchor) => (
      anchor.effect === effect.id
      && [...validAnchorsByObservedId.values()].some((anchors) => anchors.includes(anchor))
    ));
    if (!hasReferencedAnchor) {
      projectionDiagnostics.push(diag(
        effect,
        'NAVIGATION_EFFECT_CONTRACT_MISSING',
        `external-home effect ${effect.id ?? '<missing>'} must have a matching navigation anchor`,
      ));
    }
  }

  for (const item of observed) {
    if (item.kind !== 'effect' || !item.effectHint) continue;
    if (!effectsById.has(item.effectHint)) {
      diagnostics.push({
        file: path.join(root, item.file),
        line: item.line,
        column: 1,
        code: 'NAVIGATION_EFFECT_CONTRACT_MISSING',
        message: `${item.id} uses runtime effect ${item.effectHint}, but no matching (effect ...) contract exists`,
      });
      continue;
    }
    const anchors = anchorsForObservedItem(validAnchorsByObservedId, item);
    if (!anchors.some((anchor) => anchor.effect === item.effectHint)) {
      projectionDiagnostics.push({
        file: path.join(root, item.file),
        line: item.line,
        column: 1,
        code: 'NAVIGATION_EFFECT_CONTRACT_MISSING',
        message: `${item.id} uses runtime effect ${item.effectHint}, but no matching effect-site anchor references it`,
      });
    }
  }

}

export function behaviorNavigationSourceUnits(items) {
  return items
    .map((item) => ({
      semantic_id: item.semantic_id ?? item.id,
      legacy_id: (item.stability ?? 'line-bound') === 'line-bound' ? (item.legacy_id ?? item.id) : null,
      kind: item.kind,
      role: item.role ?? roleForObservedKind(item.kind),
      file: item.file,
      symbol: item.symbol ?? null,
      effect: item.effectHint ?? null,
      stability: item.stability ?? 'line-bound',
    }))
    .sort((a, b) => (
      `${a.semantic_id}\0${a.legacy_id}\0${a.kind}\0${a.file}`
        .localeCompare(`${b.semantic_id}\0${b.legacy_id}\0${b.kind}\0${b.file}`)
    ));
}

export function behaviorNavigationSourceHash({ projectId, root, sourceUnits }) {
  const payload = {
    project_id: projectId,
    root: path.resolve(root),
    scanner_version: BEHAVIOR_UNIVERSE_SCANNER_VERSION,
    source_units: sourceUnits,
  };
  return crypto.createHash('sha256').update(JSON.stringify(payload)).digest('hex');
}

function compiledAnchorToDeclaredAnchor({ root, file, anchor, index }) {
  return {
    file,
    relFile: normalizeRel(root, file),
    line: Number(anchor?.line) || 1,
    behaviorId: anchor?.behavior_id ?? anchor?.behaviorId ?? `compiled-navigation-${index + 1}`,
    role: anchor?.role ?? null,
    observed: anchor?.semantic_id ?? anchor?.id ?? anchor?.observed_id ?? null,
    semanticId: anchor?.semantic_id ?? null,
    legacyObservedId: anchor?.legacy_observed_id ?? anchor?.observed_id ?? null,
    targetFile: anchor?.file ?? null,
    symbol: anchor?.symbol ?? null,
    effect: anchor?.effect ?? null,
    source: 'compiled',
  };
}

function anchorObservedMatchesItem(anchor, item) {
  const patterns = uniqueStrings([
    anchor.semanticId,
    anchor.observed,
    anchor.legacyObservedId,
  ].filter(Boolean));
  return patterns.some((pattern) => observedMatchesPattern(item, pattern));
}

function observedMatchesPattern(item, pattern) {
  return observedIdentityKeys(item).some((id) => matchPattern(id, pattern));
}

function matchesAnyObserved(item, patterns) {
  return patterns.some((pattern) => observedMatchesPattern(item, pattern));
}

function observedIdentityKeys(item) {
  return uniqueStrings([
    item.semantic_id,
    item.id,
    item.legacy_id,
  ].filter(Boolean));
}

function anchorsForObservedItem(validAnchorsByObservedId, item) {
  return uniqueObjects(observedIdentityKeys(item).flatMap((key) => validAnchorsByObservedId.get(key) ?? []));
}

function effectiveObservedId(item) {
  return item.semantic_id ?? item.id;
}

function anchorMatchesObserved(root, anchor, item) {
  const expectedRole = navigationRoleFor(item);
  if (expectedRole && !navigationRoleCompatible(anchor.role, expectedRole, item)) return false;
  if (anchor.targetFile && anchor.targetFile !== item.file) return false;
  if (item.symbol && anchor.symbol !== item.symbol) return false;
  if (item.kind === 'effect' && item.effectHint && anchor.effect !== item.effectHint) return false;
  return true;
}

function navigationRoleCompatible(anchorRole, expectedRole, item) {
  if (anchorRole === expectedRole) return true;
  if (anchorRole === 'entry' && ['effect', 'route', 'mcp-tool', 'cli'].includes(item.kind)) return true;
  return false;
}

function isNavigationRisk(item, navigationLevel = 'risk') {
  if (item.kind === 'effect' && item.scope === 'external-home') return true;
  if (item.kind === 'effect' && Boolean(item.effectHint)) return true;
  return navigationLevel === 'risk' && NAVIGATION_RISK_KINDS.has(item.kind);
}

function navigationRoleFor(item) {
  if (item.role) return item.role;
  return roleForObservedKind(item.kind);
}

function roleForObservedKind(kind) {
  if (kind === 'mcp-tool') return 'tool';
  if (kind === 'background-task') return 'scheduler';
  if (kind === 'effect') return 'effect-site';
  if (kind === 'cli') return 'entry';
  if (NAVIGATION_RISK_KINDS.has(kind)) return kind;
  return null;
}

function fileExists(root, relOrAbs) {
  const file = path.isAbsolute(relOrAbs) ? relOrAbs : path.join(root, relOrAbs);
  return fs.existsSync(file);
}

function normalizeObservedId(id) {
  return id.replace(/\\/g, '/').replace(/\s+/g, '-');
}

function slug(value) {
  return value
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/_/g, '-')
    .toLowerCase();
}

function arrayOrEmpty(value) {
  return Array.isArray(value) ? value : [];
}

function uniqueStrings(values) {
  return [...new Set(values)].sort();
}

function uniqueObjects(values) {
  return [...new Set(values)];
}

function matchesAny(id, patterns) {
  return patterns.some((pattern) => matchPattern(id, pattern));
}

function declaredEffectCoversObserved(effect, observed) {
  if (!effect.operation || effect.operation !== observed.operation) return false;
  if (observed.scope === 'external-home') return effect.scope === 'external-home';
  return effect.scope === 'repo' || effect.scope === 'runtime' || effect.scope === 'repo-or-runtime';
}

function matchPattern(value, pattern) {
  if (pattern === value) return true;
  const escaped = String(pattern)
    .replace(/[.+^${}()|[\]\\]/g, '\\$&')
    .replace(/\*/g, '.*');
  return new RegExp(`^${escaped}$`).test(value);
}

function diag(row, code, message) {
  return {
    file: row.file,
    line: row.line ?? 1,
    column: 1,
    code,
    message,
  };
}
