#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const json = process.argv.includes('--json');
const selfTest = process.argv.includes('--self-test');

const DEFAULT_RELATIVE_SCAN_DIRS = [
  '.missiond/v3/runtime/context-gather-worker',
  '.missiond/v3/runtime/shared-artifacts',
  '.missiond/v3/runtime/artifacts',
];

const RUNTIME_SUBDIRS = [
  'context-gather-worker',
  'shared-artifacts',
  'artifacts',
  'jarvis-artifacts',
];

const MAX_FILE_BYTES = 2 * 1024 * 1024;
const TEXT_EXTENSIONS = new Set([
  '.json',
  '.jsonl',
  '.md',
  '.txt',
  '.lisp',
  '.sexp',
  '.yaml',
  '.yml',
  '.log',
]);

const SECRET_REF_PREFIXES = [
  'secret-store://',
  'vault://',
  'op://',
  'env://',
  'env:',
];

const SECRET_PATTERNS = [
  {
    code: 'SSH_PASS_LITERAL',
    regex: /\bsshpass\s+-p\s+(?:"[^"\n]{3,}"|'[^'\n]{3,}'|[^\s'"`]{3,})/gi,
  },
  {
    code: 'BEARER_TOKEN_LITERAL',
    regex: /\bBearer\s+[A-Za-z0-9._~+/=-]{16,}/g,
  },
  {
    code: 'PRIVATE_KEY_LITERAL',
    regex: /-----BEGIN [A-Z ]*PRIVATE KEY-----/g,
  },
  {
    code: 'KEY_VALUE_SECRET_LITERAL',
    regex:
      /["']?\b(password|passwd|pwd|token|api[_-]?key|secret|authorization)\b["']?\s*[:=]\s*["']?[^"'\s,;)\\]{6,}/gi,
  },
  {
    code: 'OPENAI_STYLE_TOKEN_LITERAL',
    regex: /\bsk-[A-Za-z0-9_-]{16,}/g,
  },
];

function scanRootsFromEnv() {
  const roots = new Set(DEFAULT_RELATIVE_SCAN_DIRS.map((dir) => path.resolve(dir)));
  const runtimeDir = (process.env.MISSIOND_RUNTIME_DIR ?? '').trim();
  if (runtimeDir) {
    for (const subdir of RUNTIME_SUBDIRS) {
      roots.add(path.resolve(runtimeDir, subdir));
    }
  }
  return [...roots];
}

function isProbablyTextFile(file) {
  const ext = path.extname(file).toLowerCase();
  return TEXT_EXTENSIONS.has(ext) || ext === '';
}

function walk(root) {
  const files = [];
  if (!fs.existsSync(root)) return files;
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    let stat;
    try {
      stat = fs.lstatSync(current);
    } catch {
      continue;
    }
    if (stat.isSymbolicLink()) continue;
    if (stat.isDirectory()) {
      let entries = [];
      try {
        entries = fs.readdirSync(current).map((entry) => path.join(current, entry));
      } catch {
        continue;
      }
      stack.push(...entries);
    } else if (stat.isFile() && stat.size <= MAX_FILE_BYTES && isProbablyTextFile(current)) {
      files.push(current);
    }
  }
  return files.sort();
}

function lineAndColumn(text, index) {
  const prefix = text.slice(0, index);
  const parts = prefix.split('\n');
  return {
    line: parts.length,
    column: parts[parts.length - 1].length + 1,
  };
}

function isAllowedSecretRef(matchText) {
  const lower = matchText.toLowerCase();
  if (lower.includes('<redacted')) return true;
  return SECRET_REF_PREFIXES.some((prefix) => lower.includes(prefix));
}

function scanText(file, text) {
  const diagnostics = [];
  for (const pattern of SECRET_PATTERNS) {
    pattern.regex.lastIndex = 0;
    for (const match of text.matchAll(pattern.regex)) {
      const matchText = match[0] ?? '';
      if (!matchText || isAllowedSecretRef(matchText)) continue;
      const loc = lineAndColumn(text, match.index ?? 0);
      diagnostics.push({
        code: pattern.code,
        file,
        line: loc.line,
        column: loc.column,
        message:
          'runtime/context artifact contains credential-like literal; store only secret refs or redacted fingerprints',
      });
    }
  }
  return diagnostics;
}

function runScan(roots) {
  const diagnostics = [];
  const files = [];
  for (const root of roots) {
    for (const file of walk(root)) {
      files.push(file);
      let text = '';
      try {
        text = fs.readFileSync(file, 'utf8');
      } catch (error) {
        diagnostics.push({
          code: 'READ_FAILED',
          file,
          message: error.message,
        });
        continue;
      }
      diagnostics.push(...scanText(file, text));
    }
  }
  return {
    ok: diagnostics.length === 0,
    schema: 'missiond.runtime-evidence-redaction-check.v1',
    roots,
    scanned_files: files.length,
    diagnostics,
  };
}

function runSelfTest() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-redaction-check-'));
  const root = path.join(tmp, 'context-gather-worker');
  fs.mkdirSync(root, { recursive: true });
  fs.writeFileSync(
    path.join(root, 'bad.json'),
    JSON.stringify({
      password: 'plain-password',
      ok_ref: 'secret-store://infra/bwg-vps/tunnel-ssh',
    }),
  );
  const result = runScan([root]);
  fs.rmSync(tmp, { recursive: true, force: true });
  return {
    ...result,
    ok: !result.ok && result.diagnostics.some((diag) => diag.code === 'KEY_VALUE_SECRET_LITERAL'),
    self_test: true,
  };
}

function main() {
  const result = selfTest ? runSelfTest() : runScan(scanRootsFromEnv());
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(`runtime evidence redaction check OK (${result.scanned_files} files)`);
  } else {
    console.error(JSON.stringify(result, null, 2));
  }
  process.exit(result.ok ? 0 : 1);
}

main();
