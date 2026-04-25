#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-architecture-lisp.mjs [--all-v2] [--no-structure] [--json] <files...>

Checks MissionD architecture Lisp files:
  - checks (), [], strings, escapes, and ; comments
  - reports file:line:column for reader errors
  - validates recursive architecture shape for files declaring
    (recursive-architecture-contract ...)
`;

function main() {
  const args = process.argv.slice(2);
  let checkStructure = true;
  let json = false;
  let allV2 = false;
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
    } else {
      inputArgs.push(arg);
    }
  }

  const cwd = process.cwd();
  const files = unique([
    ...(allV2 ? listV2LispFiles(cwd) : []),
    ...expandInputs(inputArgs, cwd),
  ]);

  if (files.length === 0) {
    console.error(usage);
    process.exit(2);
  }

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

  if (json) {
    console.log(JSON.stringify({ ok: diagnostics.length === 0, files: parsedCount, diagnostics }, null, 2));
  } else if (diagnostics.length === 0) {
    console.log(`architecture-lisp check OK (${parsedCount} files)`);
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
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
