#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

let repo = process.cwd();
repo = runGit(['rev-parse', '--show-toplevel']).stdout.trim();
const args = process.argv.slice(2);
const action = args.shift();

function usage(exitCode = 1) {
  const text = `missiond work-order helper

Usage:
  node scripts/missiond-work-order.mjs start "objective" [--id <id>]
  node scripts/missiond-work-order.mjs verify --staged --id <id>
  node scripts/missiond-work-order.mjs verify --commit <sha> [--id <id>]
  node scripts/missiond-work-order.mjs commit --id <id> --message "message"

Code changes must be covered by .missiond/work-orders/<id>/plan.lisp :write-scope and commits must carry:
  MissionD-Work-Order: <id>`;
  console.error(text);
  process.exit(exitCode);
}

if (!action || action === '--help' || action === '-h') usage(action ? 0 : 1);

class VerifyError extends Error {
  constructor(message, details) {
    super(message);
    this.details = details;
  }
}

try {
  if (action === 'start') startWorkOrder(args);
  else if (action === 'verify') verifyWorkOrder(args);
  else if (action === 'commit') commitWorkOrder(args);
  else usage();
} catch (err) {
  console.error(`missiond work-order ${action} failed: ${err.message}`);
  if (err.details) {
    console.error(JSON.stringify(err.details, null, 2));
  }
  process.exit(1);
}

function startWorkOrder(rest) {
  const opts = parseOpts(rest);
  const objective = opts._.join(' ').trim() || opts.objective;
  if (!objective) throw new Error('start requires an objective');
  const id = sanitizeId(opts.id || `wo-${new Date().toISOString().replace(/[-:.TZ]/g, '').slice(0, 14)}-${objective.slice(0, 32)}`);
  const dir = path.join(repo, '.missiond/work-orders', id);
  fs.mkdirSync(dir, { recursive: true });
  writeIfMissing(path.join(dir, 'intent.lisp'), `(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "${id}"
  :objective ${JSON.stringify(objective)}
  :source external-codex
  :status draft
  :unknowns []
  :evidence_refs []
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
`);
  writeIfMissing(path.join(dir, 'plan.lisp'), `(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "${id}"
  :intent "${id}"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "${id}-shard-default"
       :read_scope ["."]
       :write_scope []
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
`);
  writeIfMissing(path.join(dir, 'audit.lisp'), `(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "${id}"
  :events ((event created :at "${new Date().toISOString()}" :actor missiond-work-order)))
`);
  console.log(JSON.stringify({ ok: true, id, dir: path.relative(repo, dir), next: `edit ${path.relative(repo, path.join(dir, 'plan.lisp'))} write_scope, then run verify` }, null, 2));
}

function verifyWorkOrder(rest) {
  const opts = parseOpts(rest);
  const staged = Boolean(opts.staged);
  const commit = opts.commit;
  const id = opts.id || process.env.MISSIOND_WORK_ORDER || (commit ? workOrderIdFromCommit(commit) : null);
  const changed = staged ? stagedFiles() : commit ? commitFiles(commit) : usage();
  const codeFiles = changed.filter(isCodeLikeChange);
  if (codeFiles.length === 0) {
    console.log(JSON.stringify({ ok: true, id, changed, codeFiles, reason: 'no code-like staged/commit changes' }, null, 2));
    return;
  }
  if (!id) {
    throw new VerifyError(
      'code changes require an explicit work-order id',
      {
        staged_code_files: codeFiles,
        matched_work_order: null,
        missing: ['MISSIOND_WORK_ORDER or --id'],
        note: 'pre-commit cannot inspect the final commit message trailer; commit-msg validates MissionD-Work-Order after the message exists.',
        suggested_commands: [
          'node scripts/missiond-work-order.mjs start "describe the change"',
          'MISSIOND_WORK_ORDER=<id> node scripts/missiond-work-order.mjs verify --staged',
          'node scripts/missiond-work-order.mjs commit --id <id> --message "message"',
        ],
      },
    );
  }
  const workOrder = loadWorkOrder(id);
  const scopes = extractWriteScopes(workOrder.planText);
  if (scopes.length === 0) {
    throw new VerifyError(
      `${workOrder.planRel} has no :write_scope entries`,
      {
        staged_code_files: codeFiles,
        matched_work_order: id,
        plan: workOrder.planRel,
        missing: ['write_scope'],
        suggested_plan_patch:
          ':write_scope ["path/to/file-or-directory"] ; must cover every code-like changed file',
      },
    );
  }
  const uncovered = codeFiles.filter((file) => !isCovered(file, scopes));
  if (uncovered.length > 0) {
    throw new VerifyError(
      'write_scope does not cover all changed code files',
      {
        staged_code_files: codeFiles,
        matched_work_order: id,
        plan: workOrder.planRel,
        write_scope: scopes,
        uncovered_files: uncovered,
        suggested_commands: [
          `edit ${workOrder.planRel} and add the missing files/directories to :write_scope`,
          `MISSIOND_WORK_ORDER=${id} node scripts/missiond-work-order.mjs verify --staged`,
        ],
      },
    );
  }
  const result = {
    ok: true,
    id,
    changed,
    codeFiles,
    write_scope: scopes,
    intent: workOrder.intentRel,
    plan: workOrder.planRel,
    accepted_shard_id: extractAcceptedShardIds(workOrder.planText),
  };
  console.log(JSON.stringify(result, null, 2));
}

function commitWorkOrder(rest) {
  const opts = parseOpts(rest);
  if (!opts.id) throw new Error('commit requires --id');
  const message = opts.message || opts.m;
  if (!message) throw new Error('commit requires --message');
  verifyWorkOrder(['--staged', '--id', opts.id]);
  const commit = runGit(
    ['commit', '-m', message, '-m', `MissionD-Work-Order: ${opts.id}`],
    { env: { ...process.env, MISSIOND_WORK_ORDER: opts.id } },
  );
  process.stdout.write(commit.stdout);
  process.stderr.write(commit.stderr);
}

function parseOpts(tokens) {
  const opts = { _: [] };
  for (let i = 0; i < tokens.length; i += 1) {
    const token = tokens[i];
    if (!token.startsWith('--')) {
      opts._.push(token);
      continue;
    }
    const key = token.slice(2).replace(/-([a-z])/g, (_, ch) => ch.toUpperCase());
    const next = tokens[i + 1];
    if (!next || next.startsWith('--')) {
      opts[key] = true;
    } else {
      opts[key] = next;
      i += 1;
    }
  }
  return opts;
}

function loadWorkOrder(id) {
  const safeId = sanitizeId(id);
  const dir = path.join(repo, '.missiond/work-orders', safeId);
  const intent = path.join(dir, 'intent.lisp');
  const plan = path.join(dir, 'plan.lisp');
  if (!fs.existsSync(intent)) throw new Error(`missing ${path.relative(repo, intent)}`);
  if (!fs.existsSync(plan)) throw new Error(`missing ${path.relative(repo, plan)}`);
  return {
    dir,
    intentRel: path.relative(repo, intent),
    planRel: path.relative(repo, plan),
    planText: fs.readFileSync(plan, 'utf8'),
  };
}

function extractWriteScopes(planText) {
  const scopes = new Set();
  for (const match of planText.matchAll(/:write[-_]scope\s+\[([^\]]*)\]/g)) {
    for (const item of match[1].matchAll(/"([^"]+)"/g)) scopes.add(normalizePath(item[1]));
  }
  return [...scopes].filter(Boolean);
}

function extractAcceptedShardIds(planText) {
  return [...planText.matchAll(/:accepted[-_]shard[-_]id\s+"([^"]+)"/g)].map((m) => m[1]);
}

function isCovered(file, scopes) {
  const normalized = normalizePath(file);
  return scopes.some((scope) => {
    if (scope === '.' || scope === './' || scope === '*') return true;
    if (scope.endsWith('/**')) return normalized.startsWith(scope.slice(0, -2));
    if (scope.endsWith('/')) return normalized.startsWith(scope);
    if (scope.includes('*')) return wildcardToRegExp(scope).test(normalized);
    return normalized === scope || normalized.startsWith(`${scope}/`);
  });
}

function wildcardToRegExp(pattern) {
  const escaped = pattern.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*');
  return new RegExp(`^${escaped}$`);
}

function isCodeLikeChange(file) {
  if (file.startsWith('.missiond/work-orders/')) return false;
  if (file.includes('/runtime/')) return false;
  return /\.(rs|ts|tsx|js|mjs|cjs|py|go|java|kt|swift|sql|toml|ya?ml|json|sh|bash|zsh)$/.test(file);
}

function stagedFiles() {
  return splitLines(runGit(['diff', '--cached', '--name-only']).stdout);
}

function commitFiles(commit) {
  return splitLines(runGit(['diff-tree', '--no-commit-id', '--name-only', '-r', commit]).stdout);
}

function workOrderIdFromCommit(commit) {
  const msg = runGit(['log', '-1', '--format=%B', commit]).stdout;
  const match = msg.match(/^MissionD-Work-Order:\s*(\S+)/m);
  return match ? match[1] : null;
}

function runGit(args, opts = {}) {
  const result = spawnSync('git', args, {
    cwd: repo || process.cwd(),
    encoding: 'utf8',
    env: opts.env || process.env,
  });
  if (result.status !== 0) throw new Error(`git ${args.join(' ')} failed: ${result.stderr || result.stdout}`);
  return result;
}

function splitLines(text) {
  return text.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
}

function sanitizeId(id) {
  return String(id).replace(/[^A-Za-z0-9._-]/g, '-').replace(/-+/g, '-').slice(0, 96);
}

function normalizePath(value) {
  return String(value).replace(/\\/g, '/').replace(/^\.\//, '');
}

function writeIfMissing(file, text) {
  if (!fs.existsSync(file)) fs.writeFileSync(file, text);
}
