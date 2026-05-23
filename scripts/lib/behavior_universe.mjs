import fs from 'node:fs';
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

const TEST_FILE_RE = /(^|\/)(tests?|__tests__)(\/|$)|(_test|\.test|\.spec)\.(rs|js|mjs|cjs|ts|tsx|py)$/;

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
  return observed.sort((a, b) => a.id.localeCompare(b.id));
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

  const behaviors = universes.flatMap((u) => u.behaviors);
  const effects = universes.flatMap((u) => u.effects);
  const tombstones = universes.flatMap((u) => u.tombstones);

  return {
    projectId,
    files: lispFiles,
    universes,
    behaviors,
    effects,
    tombstones,
    diagnostics,
  };
}

export function validateBehaviorClosure(root, {
  projectId = path.basename(root),
  missiondV3 = false,
  includeTests = false,
} = {}) {
  const declared = loadDeclaredBehaviorUniverse(root, { projectId, missiondV3 });
  const observed = scanObservedUniverse(root, { projectId, includeTests });
  const diagnostics = [...declared.diagnostics];

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
  const observedIds = new Set(observed.map((item) => item.id));

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
      if (observedIds.has(observedId)) continue;
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
    if (matchesAny(item.id, tombstonePatterns)) continue;
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
    if (matchesAny(item.id, behaviorPatterns) || matchesAny(item.id, tombstonePatterns)) continue;
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

  return {
    ok: diagnostics.length === 0,
    projectId,
    observed,
    declared,
    diagnostics,
  };
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
  };
}

function parseBehavior(root, file, node) {
  const props = readKeywordProps(node, { start: 1 });
  return {
    file,
    relFile: normalizeRel(root, file),
    line: node.loc?.line ?? 1,
    id: textProp(props, ':id'),
    kind: textProp(props, ':kind'),
    owner: textProp(props, ':owner'),
    observed: arrayProp(props, ':observed'),
    code: arrayProp(props, ':code'),
    effects: arrayProp(props, ':effects'),
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

function scanRustFile({ observed, source, rel, projectId }) {
  const lines = source.split(/\r?\n/);
  let inCfgTest = false;
  lines.forEach((line, idx) => {
    if (/^\s*#\[cfg\(test\)\]/.test(line)) inCfgTest = true;
    if (inCfgTest) return;
    if (/^\s*\/\//.test(line)) return;
    const lineNo = idx + 1;
    const context = nearby(lines, idx);
    const worker = line.match(/\bimpl\s+(?:super::)?BackgroundWorker\s+for\s+([A-Za-z0-9_]+)/);
    if (worker) {
      pushObserved(observed, 'worker', `worker:${slug(worker[1])}`, rel, lineNo, 'rust-background-worker', projectId);
    }
    if (/\btokio::time::interval\b|\bstd::thread::spawn\b/.test(line)) {
      pushObserved(observed, 'scheduler', `scheduler:${rel}:${lineNo}`, rel, lineNo, 'rust-scheduler', projectId);
    }
    if (/\btokio::spawn\b/.test(line)) {
      pushObserved(observed, 'background-task', `background-task:${rel}:${lineNo}`, rel, lineNo, 'rust-tokio-spawn', projectId);
    }
    const tool = line.match(/ToolDefinition::new\(\s*"([^"]+)"/);
    if (tool) {
      pushObserved(observed, 'mcp-tool', `mcp-tool:${tool[1]}`, rel, lineNo, 'rust-mcp-tool-definition', projectId);
    }
    if (/\b(route|Router::new|\.route)\s*\(/.test(line)) {
      pushObserved(observed, 'route', `route:${rel}:${lineNo}`, rel, lineNo, 'rust-route', projectId);
    }
    if (/\b(sqlx::query|query_as!?\(|INSERT\s+INTO|UPDATE\s+|DELETE\s+FROM)\b/i.test(line)) {
      pushObserved(observed, 'db-write', `db-write:${rel}:${lineNo}`, rel, lineNo, 'rust-db-mutation-or-query', projectId);
    }
    if (/\b(Command::new|std::process::Command|\.spawn\(\))\b/.test(line) && !line.includes('tokio::spawn')) {
      pushObserved(observed, 'subprocess', `subprocess:${rel}:${lineNo}`, rel, lineNo, 'rust-subprocess', projectId);
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
      pushObserved(observed, 'effect', `effect:fs-${fsOp}:${rel}:${lineNo}`, rel, lineNo, `rust-fs-${fsOp}`, projectId, {
        operation: fsOp,
        scope,
        guard: /\bcontext::effects\b|\beffects::(write_text|atomic_write_text|append_text|remove_file)\b/.test(context),
      });
    }
  });
}

function scanJsTsFile({ observed, source, rel, projectId }) {
  const lines = source.split(/\r?\n/);
  lines.forEach((line, idx) => {
    const lineNo = idx + 1;
    const context = nearby(lines, idx);
    if (fixtureContext(context)) return;
    if (/export\s+(async\s+)?function\s+(GET|POST|PUT|PATCH|DELETE)\b|router\.(get|post|put|patch|delete)\s*\(/.test(line)) {
      pushObserved(observed, 'route', `route:${rel}:${lineNo}`, rel, lineNo, 'js-route', projectId);
    }
    if (/\b(setInterval|setTimeout|cron\.schedule|node-cron|scheduleJob)\b/.test(line)) {
      pushObserved(observed, 'scheduler', `scheduler:${rel}:${lineNo}`, rel, lineNo, 'js-scheduler', projectId);
    }
    if (/\b(fs\.writeFileSync|fs\.writeFile|writeFileSync|writeFile|appendFileSync|appendFile|rmSync|unlinkSync|renameSync|rename)\b/.test(line)) {
      const op = /append/.test(line) ? 'append' : /rm|unlink/.test(line) ? 'delete' : /rename/.test(line) ? 'rename' : 'write';
      pushObserved(observed, 'effect', `effect:fs-${op}:${rel}:${lineNo}`, rel, lineNo, `js-fs-${op}`, projectId, {
        operation: op,
        scope: externalHomeContext(context) ? 'external-home' : 'repo-or-runtime',
        guard: false,
      });
    }
    if (/\b(child_process|spawnSync|spawn\(|execFile|exec\()\b/.test(line)) {
      pushObserved(observed, 'subprocess', `subprocess:${rel}:${lineNo}`, rel, lineNo, 'js-subprocess', projectId);
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
    if (fixtureContext(context)) return;
    if (/\b(argparse\.ArgumentParser|click\.command|typer\.Typer)\b/.test(line)) {
      pushObserved(observed, 'cli', `cli:${rel}:${lineNo}`, rel, lineNo, 'python-cli', projectId);
    }
    if (/\b(schedule\.every|APScheduler|BackgroundScheduler|asyncio\.create_task)\b/.test(line)) {
      pushObserved(observed, 'scheduler', `scheduler:${rel}:${lineNo}`, rel, lineNo, 'python-scheduler', projectId);
    }
    if (/\b(open\(|Path\(.*\)\.write_text|write_bytes|os\.remove|os\.rename)\b/.test(line)) {
      const op = /remove|unlink/.test(line) ? 'delete' : /rename/.test(line) ? 'rename' : 'write';
      pushObserved(observed, 'effect', `effect:fs-${op}:${rel}:${lineNo}`, rel, lineNo, `python-fs-${op}`, projectId, {
        operation: op,
        scope: externalHomeContext(context) ? 'external-home' : 'repo-or-runtime',
        guard: false,
      });
    }
    if (/\bsubprocess\.(run|Popen|call|check_call|check_output)\b/.test(line)) {
      pushObserved(observed, 'subprocess', `subprocess:${rel}:${lineNo}`, rel, lineNo, 'python-subprocess', projectId);
    }
    if (/\b(requests\.|httpx\.|aiohttp\.)/.test(line)) {
      pushObserved(observed, 'network', `network:${rel}:${lineNo}`, rel, lineNo, 'python-network', projectId);
    }
  });
}

function rustFsOperation(line) {
  if (/\b(append_text|OpenOptions::new)\b/.test(line)) return 'append';
  if (/\b(tokio::fs::remove_file|std::fs::remove_file|fs::remove_file)\b/.test(line)) return 'delete';
  if (/\b(tokio::fs::rename|std::fs::rename|fs::rename)\b/.test(line)) return 'rename';
  if (/\b(std::fs::write|tokio::fs::write|fs::write|File::create|atomic_write_text|write_text)\b/.test(line)) return 'write';
  return null;
}

function pushObserved(observed, kind, id, file, line, detector, projectId, extra = {}) {
  observed.push({
    projectId,
    id: normalizeObservedId(id),
    kind,
    file,
    line,
    detector,
    ...extra,
  });
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

function normalizeObservedId(id) {
  return id.replace(/\\/g, '/').replace(/\s+/g, '-');
}

function slug(value) {
  return value
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/_/g, '-')
    .toLowerCase();
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
