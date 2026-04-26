#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-architecture-lisp.mjs [--all-v2] [--no-structure] [--json] [--dry-fixture] <files...>

Checks MissionD architecture Lisp files:
  - checks (), [], strings, escapes, and ; comments
  - reports file:line:column for reader errors
  - validates recursive architecture shape for files declaring
    (recursive-architecture-contract ...)
  - validates source-index entries (R015 / R016 / R017 / R018) for
    intent-pillar-source-index.lisp:
      * each (section-entry ...) must carry :section-id, :source-file,
        :local-path, :status (R015)
      * :section-id must be globally unique inside the file (R016)
      * every :source-file path must exist on disk (R017)
      * every :source-file path must live under .missiond/v2/ (R018)
      * if :compression-safe? appears, value must be one of
        true|false|yes|no|safe|unsafe|defer

Shard discovery:
  - When --all-v2 is set, all .lisp files under .missiond/v2/ are scanned.
  - When intent-pillar-source-index.lisp is in the input set (directly or
    via --all-v2), every distinct :source-file it references is auto-added
    to the scan list — no shard path is hardcoded in this script.

Use --dry-fixture to run an internal fixture-based self-test that
exercises the source-index checker on synthetic inputs and exits.
`;

function main() {
  const args = process.argv.slice(2);
  let checkStructure = true;
  let json = false;
  let allV2 = false;
  let dryFixture = false;
  const inputArgs = [];

  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--no-structure') {
      checkStructure = false;
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--all-v2') {
      allV2 = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      inputArgs.push(arg);
    }
  }

  if (dryFixture) {
    runSourceIndexFixtures();
    return;
  }

  const cwd = process.cwd();
  const initialFiles = unique([
    ...(allV2 ? listV2LispFiles(cwd) : []),
    ...expandInputs(inputArgs, cwd),
  ]);

  if (initialFiles.length === 0) {
    console.error(usage);
    process.exit(2);
  }

  // ── Pass 1: pre-parse intent-pillar-source-index.lisp (if present) to
  // auto-discover any shard files it references via :source-file. The set is
  // unioned with --all-v2 / explicit inputs and deduplicated. No shard path
  // is hardcoded — discovery flows entirely from source-index data.
  const discovered = new Set(initialFiles);
  let discoveredFromSourceIndex = 0;
  for (const file of initialFiles) {
    if (path.basename(file) !== 'intent-pillar-source-index.lisp') continue;
    try {
      const source = fs.readFileSync(file, 'utf8');
      const forms = parse(source, file);
      const refs = collectSourceFileRefs(forms);
      for (const ref of refs) {
        const abs = path.resolve(cwd, ref);
        if (!discovered.has(abs)) {
          discovered.add(abs);
          discoveredFromSourceIndex += 1;
        }
      }
    } catch {
      // Reader/parse errors will resurface in pass 2; silently skip
      // discovery here so we don't double-report.
    }
  }

  const files = [...discovered];

  const diagnostics = [];
  let parsedCount = 0;

  for (const file of files) {
    try {
      const source = fs.readFileSync(file, 'utf8');
      parsedCount += 1;
      checkReaderBalance(source, file);
      if (checkStructure && source.includes('(recursive-architecture-contract')) {
        const forms = parse(source, file);
        validateArchitectureShape(file, forms, diagnostics);
      }
      if (
        checkStructure &&
        path.basename(file) === 'intent-pillar-source-index.lisp'
      ) {
        const forms = parse(source, file);
        validateSourceIndex(file, forms, diagnostics, { repoRoot: cwd });
      }
    } catch (err) {
      diagnostics.push({
        severity: 'error',
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        message: err.message,
      });
    }
  }

  const summary = {
    files: parsedCount,
    discoveredFromSourceIndex,
    initialFiles: initialFiles.length,
  };

  if (json) {
    console.log(
      JSON.stringify(
        { ok: diagnostics.length === 0, ...summary, diagnostics },
        null,
        2,
      ),
    );
  } else if (diagnostics.length === 0) {
    const tail =
      discoveredFromSourceIndex > 0
        ? ` (${initialFiles.length} initial + ${discoveredFromSourceIndex} shard(s) auto-discovered from source-index)`
        : '';
    console.log(`architecture-lisp check OK (${parsedCount} files)${tail}`);
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `architecture-lisp check FAILED — ${diagnostics.length} diagnostic(s) across ${parsedCount} file(s)` +
        (discoveredFromSourceIndex > 0
          ? ` (${initialFiles.length} initial + ${discoveredFromSourceIndex} shard(s) auto-discovered from source-index)`
          : ''),
    );
  }

  process.exit(diagnostics.some((d) => d.severity === 'error') ? 1 : 0);
}

function listV2LispFiles(root) {
  const dir = path.join(root, '.missiond', 'v2');
  return fs
    .readdirSync(dir)
    .filter((name) => name.endsWith('.lisp'))
    .map((name) => path.join(dir, name));
}

function expandInputs(inputs, root) {
  const out = [];
  for (const input of inputs) {
    if (input.includes('*')) {
      out.push(...expandSimpleGlob(input, root));
    } else {
      out.push(path.resolve(root, input));
    }
  }
  return out;
}

function expandSimpleGlob(pattern, root) {
  const absolute = path.resolve(root, pattern);
  const dir = path.dirname(absolute);
  const base = path.basename(absolute);
  const re = new RegExp(`^${escapeRegExp(base).replaceAll('\\*', '.*')}$`);
  return fs
    .readdirSync(dir)
    .filter((name) => re.test(name))
    .map((name) => path.join(dir, name));
}

function unique(items) {
  return [...new Set(items)];
}

function parse(source, file) {
  const p = new Parser(source, file);
  return p.parseForms(null);
}

function checkReaderBalance(source, file) {
  const stack = [];
  let inString = false;
  let esc = false;
  let comment = false;
  let line = 1;
  let column = 1;

  for (const c of source) {
    if (comment) {
      if (c === '\n') comment = false;
    } else if (inString) {
      if (esc) esc = false;
      else if (c === '\\') esc = true;
      else if (c === '"') inString = false;
    } else if (c === ';') {
      comment = true;
    } else if (c === '"') {
      inString = true;
    } else if (c === '(' || c === '[') {
      stack.push({ c, line, column });
    } else if (c === ')' || c === ']') {
      const open = stack.pop();
      const expected = c === ')' ? '(' : '[';
      if (!open || open.c !== expected) {
        const got = open ? `'${open.c}' from ${open.line}:${open.column}` : 'nothing';
        readerFail(file, line, column, `unexpected closing delimiter '${c}', matched ${got}`);
      }
    }

    if (c === '\n') {
      line += 1;
      column = 1;
    } else {
      column += 1;
    }
  }

  if (inString) {
    readerFail(file, line, column, 'unterminated string');
  }
  if (stack.length > 0) {
    const open = stack.at(-1);
    readerFail(file, open.line, open.column, `missing closing delimiter for '${open.c}'`);
  }
}

function readerFail(file, line, column, message) {
  const err = new Error(message);
  err.file = file;
  err.line = line;
  err.column = column;
  throw err;
}

class Parser {
  constructor(source, file) {
    this.source = source;
    this.file = file;
    this.i = 0;
    this.line = 1;
    this.column = 1;
  }

  parseForms(closeDelimiter) {
    const forms = [];
    while (!this.eof()) {
      this.skipSpaceAndComments();
      if (this.eof()) break;

      const c = this.peek();
      if (closeDelimiter && c === closeDelimiter) {
        this.advance();
        return forms;
      }
      if (c === ')' || c === ']') {
        this.fail(`unexpected closing delimiter '${c}'`);
      }
      forms.push(this.parseForm());
    }

    if (closeDelimiter) {
      this.fail(`missing closing delimiter '${closeDelimiter}'`);
    }
    return forms;
  }

  parseForm() {
    this.skipSpaceAndComments();
    const c = this.peek();
    if (c === '(') return this.parseList('paren', ')');
    if (c === '[') return this.parseList('bracket', ']');
    if (c === '"') return this.parseString();
    return this.parseAtom();
  }

  parseList(kind, closeDelimiter) {
    const loc = this.loc();
    this.advance();
    const children = this.parseForms(closeDelimiter);
    return { type: 'list', kind, children, loc };
  }

  parseString() {
    const loc = this.loc();
    let value = '';
    this.advance();
    while (!this.eof()) {
      const c = this.advance();
      if (c === '"') {
        return { type: 'string', value, loc };
      }
      if (c === '\\') {
        if (this.eof()) this.fail('unterminated string escape', loc);
        value += c + this.advance();
      } else {
        value += c;
      }
    }
    this.fail('unterminated string', loc);
  }

  parseAtom() {
    const loc = this.loc();
    let value = '';
    while (!this.eof()) {
      const c = this.peek();
      if (/\s/.test(c) || c === '(' || c === ')' || c === '[' || c === ']' || c === ';') {
        break;
      }
      value += this.advance();
    }
    if (value.length === 0) this.fail(`unexpected character '${this.peek()}'`);
    return { type: 'atom', value, loc };
  }

  skipSpaceAndComments() {
    while (!this.eof()) {
      const c = this.peek();
      if (/\s/.test(c)) {
        this.advance();
      } else if (c === ';') {
        while (!this.eof() && this.peek() !== '\n') this.advance();
      } else {
        break;
      }
    }
  }

  eof() {
    return this.i >= this.source.length;
  }

  peek() {
    return this.source[this.i];
  }

  advance() {
    const c = this.source[this.i++];
    if (c === '\n') {
      this.line += 1;
      this.column = 1;
    } else {
      this.column += 1;
    }
    return c;
  }

  loc() {
    return { line: this.line, column: this.column };
  }

  fail(message, loc = this.loc()) {
    const err = new Error(message);
    err.line = loc.line;
    err.column = loc.column;
    throw err;
  }
}

function validateArchitectureShape(file, forms, diagnostics) {
  for (const root of forms.filter(isList)) {
    if (!hasDirectChild(root, 'recursive-architecture-contract')) continue;

    for (const required of ['pillar-ingress', 'pillar-core', 'pillar-egress']) {
      if (!hasDirectChild(root, required)) {
        addError(diagnostics, file, root.loc, `recursive architecture file is missing (${required} ...)`);
      }
    }

    const core = directChildren(root, 'pillar-core')[0];
    if (!core) continue;

    for (const fn of directChildren(core, 'function')) {
      const name = nodeText(fn.children[1]) ?? '<unnamed>';
      for (const required of ['ingress', 'logic-core', 'egress']) {
        if (!hasDirectChild(fn, required)) {
          addError(diagnostics, file, fn.loc, `(function ${name} ...) is missing (${required} ...)`);
        }
      }
      for (const logic of directChildren(fn, 'logic-core')) {
        validateStepSequence(file, logic, diagnostics, `(function ${name})`);
      }
    }
  }
}

function validateStepSequence(file, logicCore, diagnostics, context) {
  const steps = directChildren(logicCore, 'step');
  let expected = 1;
  for (const step of steps) {
    const id = nodeText(step.children[1]);
    if (!id || !/^s\d+$/.test(id)) continue;
    const n = Number(id.slice(1));
    if (n !== expected) {
      addError(diagnostics, file, step.loc, `${context} step sequence expected s${expected}, got ${id}`);
      expected = n + 1;
    } else {
      expected += 1;
    }
  }
}

// ── source-index checker (R015 / R016 / R017 / R018) ──────────────
// Validates intent-pillar-source-index.lisp:
//   * each (section-entry ...) carries :section-id, :source-file,
//     :local-path, :status (R015)
//   * :section-id values are globally unique (R016)
//   * every :source-file path exists on disk (R017, shard-aware)
//   * every :source-file path lives under .missiond/v2/ (R018, shard-aware)
//   * if :compression-safe? appears, value is one of
//     true|false|yes|no|safe|unsafe|defer
const SOURCE_INDEX_REQUIRED_FIELDS = [
  ':section-id',
  ':source-file',
  ':local-path',
  ':status',
];

const COMPRESSION_SAFE_ALLOWED = new Set([
  'true',
  'false',
  'yes',
  'no',
  'safe',
  'unsafe',
  'defer',
]);

const V2_DIR_PREFIX = '.missiond/v2/';

function validateSourceIndex(file, forms, diagnostics, opts = {}) {
  const repoRoot = opts.repoRoot ?? process.cwd();
  const entries = collectSectionEntries(forms);
  const seenIds = new Map();
  const seenSourceFileExistence = new Map();

  for (const entry of entries) {
    const props = readKeywordProps(entry);

    for (const key of SOURCE_INDEX_REQUIRED_FIELDS) {
      if (!Object.prototype.hasOwnProperty.call(props, key)) {
        addError(
          diagnostics,
          file,
          entry.loc,
          `(section-entry ...) missing required field ${key} (R015)`,
        );
      }
    }

    const idEntry = props[':section-id'];
    if (idEntry) {
      const id = nodeText(idEntry.value);
      if (!id) {
        addError(
          diagnostics,
          file,
          idEntry.value?.loc ?? entry.loc,
          `:section-id must be a kebab-case string or atom (R015)`,
        );
      } else if (seenIds.has(id)) {
        const prior = seenIds.get(id);
        addError(
          diagnostics,
          file,
          idEntry.value.loc,
          `duplicate :section-id "${id}" (R016); first declared at ${prior.line}:${prior.column}`,
        );
      } else {
        seenIds.set(id, idEntry.value.loc);
      }
    }

    const sourceEntry = props[':source-file'];
    if (sourceEntry && sourceEntry.value) {
      validateSourceFileRef(
        file,
        sourceEntry.value,
        diagnostics,
        repoRoot,
        seenSourceFileExistence,
      );
    }

    const compEntry = props[':compression-safe?'];
    if (compEntry) {
      const raw = nodeText(compEntry.value);
      if (raw == null || !COMPRESSION_SAFE_ALLOWED.has(raw)) {
        const shown = raw == null ? '<non-atom>' : raw;
        addError(
          diagnostics,
          file,
          compEntry.value?.loc ?? entry.loc,
          `:compression-safe? value "${shown}" must be one of ${[...COMPRESSION_SAFE_ALLOWED].join('|')}`,
        );
      }
    }
  }

  // ── R017+R018 also apply to the (pillar-section-index :source-file ...)
  // header, which carries the shard/parent file pointer for the whole pillar.
  for (const psi of collectPillarSectionIndexes(forms)) {
    const props = readKeywordProps(psi);
    const sourceEntry = props[':source-file'];
    if (sourceEntry && sourceEntry.value) {
      validateSourceFileRef(
        file,
        sourceEntry.value,
        diagnostics,
        repoRoot,
        seenSourceFileExistence,
      );
    }
  }
}

// Per-source-index-file cache so we only stat each path once even when
// many section-entries share it.
function validateSourceFileRef(file, valueNode, diagnostics, repoRoot, cache) {
  const raw = nodeText(valueNode);
  if (raw == null) {
    addError(
      diagnostics,
      file,
      valueNode.loc,
      `:source-file must be a string path`,
    );
    return;
  }

  // R018: under .missiond/v2/. Compare on the declared path, not the
  // resolved one — the declared form is the contract surface that humans
  // and tools cross-reference.
  const normalized = raw.replace(/\\/g, '/');
  if (!normalized.startsWith(V2_DIR_PREFIX)) {
    addError(
      diagnostics,
      file,
      valueNode.loc,
      `:source-file "${raw}" must live under ${V2_DIR_PREFIX} (R018)`,
    );
    return;
  }

  // R017: file must exist on disk (resolved against repo root).
  if (cache.has(normalized)) {
    if (cache.get(normalized) === false) {
      addError(
        diagnostics,
        file,
        valueNode.loc,
        `:source-file "${raw}" does not exist on disk (R017)`,
      );
    }
    return;
  }
  const abs = path.resolve(repoRoot, normalized);
  let exists = false;
  try {
    exists = fs.statSync(abs).isFile();
  } catch {
    exists = false;
  }
  cache.set(normalized, exists);
  if (!exists) {
    addError(
      diagnostics,
      file,
      valueNode.loc,
      `:source-file "${raw}" does not exist on disk (R017)`,
    );
  }
}

// Walk source-index forms and return the distinct set of declared
// :source-file string values across (pillar-section-index ...) headers and
// (section-entry ...) bodies. Discovery is data-driven so adding a new
// shard is a one-line source-index edit, not a checker change.
function collectSourceFileRefs(forms) {
  const seen = new Set();
  const stack = [...forms];
  while (stack.length > 0) {
    const node = stack.pop();
    if (!node || node.type !== 'list') continue;
    const tag = head(node);
    if (tag === 'section-entry' || tag === 'pillar-section-index') {
      const props = readKeywordProps(node);
      const sourceEntry = props[':source-file'];
      const raw = sourceEntry ? nodeText(sourceEntry.value) : null;
      if (raw) seen.add(raw.replace(/\\/g, '/'));
    }
    for (const child of node.children) stack.push(child);
  }
  return [...seen];
}

function collectPillarSectionIndexes(forms) {
  const out = [];
  const stack = [...forms];
  while (stack.length > 0) {
    const node = stack.pop();
    if (!node || node.type !== 'list') continue;
    if (head(node) === 'pillar-section-index') out.push(node);
    for (const child of node.children) stack.push(child);
  }
  return out;
}

function collectSectionEntries(forms) {
  const out = [];
  const stack = [...forms];
  while (stack.length > 0) {
    const node = stack.pop();
    if (!node || node.type !== 'list') continue;
    if (head(node) === 'section-entry') {
      out.push(node);
    }
    for (const child of node.children) stack.push(child);
  }
  return out;
}

// (section-entry :key val :key val …) — return { ':key': { keyNode, value } }
// Skips the leading head atom; pairs each :keyword atom with its next sibling.
function readKeywordProps(entry) {
  const props = {};
  const children = entry.children;
  for (let i = 1; i < children.length; i++) {
    const node = children[i];
    if (node.type !== 'atom') continue;
    if (!node.value.startsWith(':')) continue;
    const key = node.value;
    const value = i + 1 < children.length ? children[i + 1] : null;
    props[key] = { keyNode: node, value };
    if (value) i += 1;
  }
  return props;
}

// ── inline dry-fixture self-test ───────────────────────────────────
// Runs the source-index checker against synthetic snippets so
// `--dry-fixture` exercises the new rules without relying on the live
// .missiond/v2/ tree. Exits 0 on success, 1 on assertion failure.
function runSourceIndexFixtures() {
  // R017/R018 fixtures need a temp repo root with controlled files. We make
  // a sandbox under the OS temp dir so tests do not depend on the live
  // .missiond/v2/ tree.
  const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), 'archlisp-fixture-'));
  const v2Dir = path.join(sandbox, '.missiond', 'v2');
  fs.mkdirSync(v2Dir, { recursive: true });
  fs.writeFileSync(path.join(v2Dir, 'intent-memory.lisp'), '(stub)');
  fs.writeFileSync(path.join(v2Dir, 'intent-flow.lisp'), '(stub)');
  // Note: no intent-missing.lisp on purpose — used by the R017 fixture.

  const fixtures = [
    {
      name: 'happy-path: full entry passes all rules',
      repoRoot: sandbox,
      source: `(source-index v2
        (pillar-section-index :pillar memory :source-file ".missiond/v2/intent-memory.lisp"
          (section-entry
            :section-id "memory.demo"
            :title "demo"
            :source-file ".missiond/v2/intent-memory.lisp"
            :local-path "pillar memory :: demo"
            :status code-aligned
            :compression-safe? true)))`,
      expectMessages: [],
    },
    {
      name: 'missing required fields surface R015',
      repoRoot: sandbox,
      source: `(source-index v2
        (section-entry
          :title "no required fields"))`,
      expectMessages: [
        'missing required field :section-id',
        'missing required field :source-file',
        'missing required field :local-path',
        'missing required field :status',
      ],
    },
    {
      name: 'duplicate section-id triggers R016',
      repoRoot: sandbox,
      source: `(source-index v2
        (section-entry
          :section-id "dup.id"
          :source-file ".missiond/v2/intent-memory.lisp"
          :local-path "pillar a"
          :status code-aligned)
        (section-entry
          :section-id "dup.id"
          :source-file ".missiond/v2/intent-flow.lisp"
          :local-path "pillar b"
          :status code-aligned))`,
      expectMessages: ['duplicate :section-id "dup.id"'],
    },
    {
      name: 'compression-safe? rejects unknown values',
      repoRoot: sandbox,
      source: `(source-index v2
        (section-entry
          :section-id "bad.compsafe"
          :source-file ".missiond/v2/intent-memory.lisp"
          :local-path "pillar x"
          :status code-aligned
          :compression-safe? maybe))`,
      expectMessages: [':compression-safe? value "maybe" must be one of'],
    },
    {
      name: 'compression-safe? accepts atom alias safe/defer/yes/no/unsafe',
      repoRoot: sandbox,
      source: `(source-index v2
        (section-entry
          :section-id "alias.safe"
          :source-file ".missiond/v2/intent-memory.lisp" :local-path "pillar x" :status code-aligned
          :compression-safe? safe)
        (section-entry
          :section-id "alias.defer"
          :source-file ".missiond/v2/intent-memory.lisp" :local-path "pillar x" :status code-aligned
          :compression-safe? defer)
        (section-entry
          :section-id "alias.yes"
          :source-file ".missiond/v2/intent-memory.lisp" :local-path "pillar x" :status code-aligned
          :compression-safe? yes)
        (section-entry
          :section-id "alias.no"
          :source-file ".missiond/v2/intent-memory.lisp" :local-path "pillar x" :status code-aligned
          :compression-safe? no)
        (section-entry
          :section-id "alias.unsafe"
          :source-file ".missiond/v2/intent-memory.lisp" :local-path "pillar x" :status code-aligned
          :compression-safe? unsafe))`,
      expectMessages: [],
    },
    {
      name: 'R017 missing :source-file path triggers does-not-exist',
      repoRoot: sandbox,
      source: `(source-index v2
        (section-entry
          :section-id "r017.missing"
          :source-file ".missiond/v2/intent-missing.lisp"
          :local-path "pillar x"
          :status code-aligned))`,
      expectMessages: ['intent-missing.lisp" does not exist on disk (R017)'],
    },
    {
      name: 'R018 :source-file outside .missiond/v2/ rejected',
      repoRoot: sandbox,
      source: `(source-index v2
        (section-entry
          :section-id "r018.outside"
          :source-file "crates/missiond-core/src/lib.rs"
          :local-path "pillar x"
          :status code-aligned))`,
      expectMessages: ['must live under .missiond/v2/ (R018)'],
    },
    {
      name: 'R018 takes precedence — out-of-tree path does not trigger R017',
      repoRoot: sandbox,
      source: `(source-index v2
        (section-entry
          :section-id "r018.short-circuits"
          :source-file "/etc/passwd"
          :local-path "pillar x"
          :status code-aligned))`,
      expectMessages: ['must live under .missiond/v2/ (R018)'],
      forbidMessages: ['(R017)'],
    },
    {
      name: 'R017 also enforced on (pillar-section-index ...) header',
      repoRoot: sandbox,
      source: `(source-index v2
        (pillar-section-index :pillar memory :source-file ".missiond/v2/intent-missing.lisp"))`,
      expectMessages: ['intent-missing.lisp" does not exist on disk (R017)'],
    },
    {
      name: 'shard auto-discovery: collectSourceFileRefs returns shard set',
      repoRoot: sandbox,
      source: `(source-index v2
        (pillar-section-index :pillar memory :source-file ".missiond/v2/intent-memory.lisp"
          (section-entry
            :section-id "memory.demo"
            :title "demo"
            :source-file ".missiond/v2/intent-memory.lisp"
            :local-path "pillar memory :: demo"
            :status code-aligned)
          (section-entry
            :section-id "memory.shard.x"
            :title "shard x"
            :source-file ".missiond/v2/intent-flow.lisp"
            :local-path "pillar memory :: shard x"
            :status code-aligned)))`,
      expectMessages: [],
      assertRefs: ['.missiond/v2/intent-memory.lisp', '.missiond/v2/intent-flow.lisp'],
    },
  ];

  let failed = 0;
  for (const fx of fixtures) {
    const file = '<fixture>';
    const diagnostics = [];
    const forms = parse(fx.source, file);
    validateSourceIndex(file, forms, diagnostics, { repoRoot: fx.repoRoot });
    const messages = diagnostics.map((d) => d.message);
    const missing = fx.expectMessages.filter(
      (needle) => !messages.some((m) => m.includes(needle)),
    );
    const forbidden = (fx.forbidMessages ?? []).filter((needle) =>
      messages.some((m) => m.includes(needle)),
    );
    const extra =
      fx.expectMessages.length === 0 && messages.length > 0
        ? messages
        : [];

    let refsOk = true;
    if (fx.assertRefs) {
      const refs = new Set(collectSourceFileRefs(forms));
      const missingRefs = fx.assertRefs.filter((r) => !refs.has(r));
      if (missingRefs.length > 0) {
        refsOk = false;
        console.error(`FAIL  ${fx.name}`);
        console.error(`  collectSourceFileRefs missing:`);
        for (const r of missingRefs) console.error(`    - ${r}`);
      }
    }

    if (missing.length > 0 || forbidden.length > 0 || extra.length > 0 || !refsOk) {
      if (missing.length > 0 || forbidden.length > 0 || extra.length > 0) {
        failed += 1;
        console.error(`FAIL  ${fx.name}`);
        if (missing.length > 0) {
          console.error(`  missing expected substrings:`);
          for (const m of missing) console.error(`    - ${m}`);
        }
        if (forbidden.length > 0) {
          console.error(`  forbidden substrings present:`);
          for (const m of forbidden) console.error(`    - ${m}`);
        }
        if (extra.length > 0) {
          console.error(`  unexpected diagnostics:`);
          for (const m of extra) console.error(`    - ${m}`);
        }
      } else if (!refsOk) {
        failed += 1;
      }
    } else {
      console.log(`OK    ${fx.name}`);
    }
  }

  // Best-effort cleanup of the sandbox; ignore errors.
  try {
    fs.rmSync(sandbox, { recursive: true, force: true });
  } catch {
    /* ignore */
  }

  if (failed > 0) {
    console.error(`\ndry-fixture: ${failed} fixture(s) failed`);
    process.exit(1);
  }
  console.log(`\ndry-fixture: ${fixtures.length} fixture(s) OK`);
  process.exit(0);
}

function isList(node) {
  return node?.type === 'list';
}

function head(node) {
  return isList(node) ? nodeText(node.children[0]) : null;
}

function nodeText(node) {
  if (!node) return null;
  if (node.type === 'atom' || node.type === 'string') return node.value;
  return null;
}

function directChildren(node, wantedHead) {
  return node.children.filter((child) => isList(child) && head(child) === wantedHead);
}

function hasDirectChild(node, wantedHead) {
  return directChildren(node, wantedHead).length > 0;
}

function addError(diagnostics, file, loc, message) {
  diagnostics.push({
    severity: 'error',
    file,
    line: loc.line,
    column: loc.column,
    message,
  });
}

function escapeRegExp(s) {
  return s.replace(/[.+?^${}()|[\]\\]/g, '\\$&');
}

main();
