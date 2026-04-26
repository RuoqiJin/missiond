import fs from 'node:fs';
import path from 'node:path';

export function parseLisp(source, file = '<memory>') {
  const parser = new Parser(source, file);
  return parser.parseForms(null);
}

export function readLispFile(file) {
  return parseLisp(fs.readFileSync(file, 'utf8'), file);
}

export function isList(node) {
  return node && node.type === 'list';
}

export function isAtom(node, value = undefined) {
  if (!node || node.type !== 'atom') return false;
  return value === undefined ? true : node.value === value;
}

export function head(node) {
  if (!isList(node) || node.children.length === 0) return null;
  const first = node.children[0];
  return first.type === 'atom' ? first.value : null;
}

export function nodeText(node) {
  if (!node) return null;
  if (node.type === 'string') return node.value;
  if (node.type === 'atom') return node.value;
  return null;
}

export function nodeBool(node) {
  const raw = nodeText(node);
  if (raw == null) return null;
  const normalized = raw.toLowerCase();
  if (['true', 'yes', 'on', 'required'].includes(normalized)) return true;
  if (['false', 'no', 'off', 'not-required', 'none'].includes(normalized)) return false;
  return null;
}

export function readKeywordProps(node, { start = 1 } = {}) {
  const props = {};
  if (!isList(node)) return props;
  const children = node.children;
  for (let i = start; i < children.length; i++) {
    const current = children[i];
    if (current.type !== 'atom' || !current.value.startsWith(':')) continue;
    const value = i + 1 < children.length ? children[i + 1] : null;
    props[current.value] = { keyNode: current, value };
    if (value) i += 1;
  }
  return props;
}

export function keywordPropText(props, key) {
  return nodeText(props[key]?.value);
}

export function keywordPropBool(props, key) {
  return nodeBool(props[key]?.value);
}

export function nodeToStringArray(node) {
  if (!node) return [];
  if (node.type === 'string' || node.type === 'atom') {
    const text = nodeText(node);
    return text == null || text === '' ? [] : [text];
  }
  if (!isList(node)) return [];
  return node.children
    .map((child) => nodeText(child))
    .filter((value) => value != null && value !== '');
}

export function listLispFilesRecursive(root) {
  const out = [];
  if (!fs.existsSync(root)) return out;
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const p = path.join(root, entry.name);
    if (entry.isDirectory()) {
      out.push(...listLispFilesRecursive(p));
    } else if (entry.isFile() && entry.name.endsWith('.lisp')) {
      out.push(p);
    }
  }
  return out;
}

export function formatLoc(file, loc) {
  return `${file}:${loc?.line ?? 1}:${loc?.column ?? 1}`;
}

// Convert a path-style glob pattern into a RegExp matching repo-relative paths.
// Supports: `*` (single segment, no `/`), `**` (any number of segments incl. zero),
// `?` (single non-`/` char). Other regex metacharacters are escaped.
export function globToRegExp(pattern) {
  const normalized = pattern.replace(/\\/g, '/').replace(/^\.\//, '').replace(/^\/+/, '');
  let regex = '';
  let i = 0;
  while (i < normalized.length) {
    const c = normalized[i];
    if (c === '*') {
      if (normalized[i + 1] === '*') {
        // `**` -> any sequence including `/`
        regex += '.*';
        i += 2;
        // swallow a following `/` so `**/foo` matches `foo` and `a/foo`
        if (normalized[i] === '/') {
          regex += '(?:|/)?';
          // we already consumed the `/`; skip nothing more to avoid double-encoding
          // but we still want a `/` in matches when one is present, which `.*` covers
          // so just step past it
          i += 0;
        }
      } else {
        // `*` -> any chars except `/`
        regex += '[^/]*';
        i += 1;
      }
    } else if (c === '?') {
      regex += '[^/]';
      i += 1;
    } else if (/[.+^${}()|[\]\\]/.test(c)) {
      regex += '\\' + c;
      i += 1;
    } else {
      regex += c;
      i += 1;
    }
  }
  return new RegExp('^' + regex + '$');
}

// Match a single repo-relative path against a glob pattern.
// A pattern without any glob metacharacters matches either the exact path
// or any file under that path when the pattern names a directory prefix
// (e.g. `crates/` or `crates` matches `crates/foo/bar.rs`).
export function pathMatchesPattern(filePath, pattern) {
  const norm = filePath.replace(/\\/g, '/').replace(/^\.\//, '').replace(/^\/+/, '');
  const pat = pattern.replace(/\\/g, '/').replace(/^\.\//, '').replace(/^\/+/, '');
  if (!/[*?]/.test(pat)) {
    if (norm === pat) return true;
    const prefix = pat.endsWith('/') ? pat : pat + '/';
    return norm.startsWith(prefix);
  }
  return globToRegExp(pat).test(norm);
}

export function pathMatchesAny(filePath, patterns) {
  if (!patterns || patterns.length === 0) return false;
  for (const pat of patterns) {
    if (pathMatchesPattern(filePath, pat)) return true;
  }
  return false;
}

export class LispReaderError extends Error {
  constructor(file, loc, message) {
    super(message);
    this.file = file;
    this.line = loc?.line ?? 1;
    this.column = loc?.column ?? 1;
  }
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

    if (closeDelimiter) this.fail(`missing closing delimiter '${closeDelimiter}'`);
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
      if (c === '"') return { type: 'string', value, loc };
      if (c === '\\') {
        if (this.eof()) this.fail('unterminated string escape', loc);
        const escaped = this.advance();
        value += escaped;
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
    throw new LispReaderError(this.file, loc, message);
  }
}
