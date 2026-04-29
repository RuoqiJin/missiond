#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  head,
  isList,
  keywordPropBool,
  keywordPropText,
  nodeToStringArray,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/render-claudecode-task.mjs [--stdout] [--force] [--brief-mode full|thin] [--shared-preamble <path>] [--out <path>] <task.lisp>
  node scripts/render-claudecode-task.mjs --brief-mode preamble [--stdout] [--force] [--out <path>]
  node scripts/render-claudecode-task.mjs --dry-fixture

Renders a MissionD task-contract v1 Lisp file into the current ClaudeCode
Markdown task-brief format. By default writes:
  .missiond/claudecode/<task-id>.md

--brief-mode thin renders only task-specific execution facts and points at a
shared preamble for boilerplate protocols. --brief-mode preamble renders the
shared boilerplate once per wave.

--dry-fixture runs self-contained smoke fixtures that render a synthetic
in-memory task contract and assert the wave26-06 + wave27-05 cross-wave
literals (advisory / dry-run only / no execution / router-backend-registry /
--backend-registry / MUST NOT switch backend / build-router-dispatch-
descriptor) all surface in the rendered output. Includes static-audit
fixtures that scan the renderer source for forbidden shell/LLM/git/network
patterns. The renderer NEVER shells out — every command in the rendered
brief is text only.
`;

function main() {
  const args = process.argv.slice(2);
  let stdout = false;
  let force = false;
  let outPath = null;
  let dryFixture = false;
  let briefMode = 'full';
  let sharedPreamble = null;
  const inputs = [];

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--stdout') {
      stdout = true;
    } else if (arg === '--force') {
      force = true;
    } else if (arg === '--out') {
      outPath = args[++i];
      if (!outPath) fail('--out requires a path');
    } else if (arg === '--brief-mode') {
      briefMode = args[++i];
      if (!['full', 'thin', 'preamble'].includes(briefMode)) fail('--brief-mode must be full, thin, or preamble');
    } else if (arg === '--shared-preamble') {
      sharedPreamble = args[++i];
      if (!sharedPreamble) fail('--shared-preamble requires a path');
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      inputs.push(arg);
    }
  }

  if (dryFixture) {
    runFixtures();
    return;
  }

  if (briefMode === 'preamble') {
    if (inputs.length !== 0) fail('--brief-mode preamble does not take a task.lisp input');
    const markdown = renderSharedPreamble();
    if (stdout) {
      process.stdout.write(markdown);
      return;
    }
    const outputPath = path.resolve(
      process.cwd(),
      outPath ?? path.join('.missiond', 'claudecode', 'shared-preamble.md'),
    );
    if (fs.existsSync(outputPath) && !force) {
      fail(`${outputPath} already exists; pass --force to overwrite`);
    }
    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    fs.writeFileSync(outputPath, markdown);
    console.log(`rendered ${path.relative(process.cwd(), outputPath)} shared preamble`);
    return;
  }

  if (inputs.length !== 1) fail(usage);

  const sourcePath = path.resolve(process.cwd(), inputs[0]);
  const task = loadSingleTask(sourcePath);
  const markdown = renderTask(task, sourcePath, { briefMode, sharedPreamble });

  if (stdout) {
    process.stdout.write(markdown);
    return;
  }

  const outputPath = path.resolve(
    process.cwd(),
    outPath ?? path.join('.missiond', 'claudecode', `${task.id}.md`),
  );
  if (fs.existsSync(outputPath) && !force) {
    fail(`${outputPath} already exists; pass --force to overwrite`);
  }
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, markdown);
  console.log(`rendered ${path.relative(process.cwd(), outputPath)} from ${path.relative(process.cwd(), sourcePath)}`);
}

export function loadSingleTask(file) {
  const forms = readLispFile(file);
  const tasks = forms.filter((form) => isList(form) && head(form) === 'task');
  if (tasks.length !== 1) fail(`${file} must contain exactly one (task ...) form; got ${tasks.length}`);
  const node = tasks[0];
  const id = node.children[1]?.value;
  const props = readKeywordProps(node, { start: 2 });
  const commitNode = props[':commit']?.value;
  const commitProps = isList(commitNode) ? readKeywordProps(commitNode, { start: 0 }) : {};
  return {
    id,
    title: keywordPropText(props, ':title') ?? id,
    kind: keywordPropText(props, ':kind') ?? 'task',
    status: keywordPropText(props, ':status') ?? 'ready',
    owner: keywordPropText(props, ':owner') ?? 'claudecode',
    goal: keywordPropText(props, ':goal') ?? '',
    dependsOn: nodeToStringArray(props[':depends-on']?.value),
    dispatchStrategy: keywordPropText(props, ':dispatch-strategy'),
    writeScope: nodeToStringArray(props[':write-scope']?.value),
    mustNotTouch: nodeToStringArray(props[':must-not-touch']?.value),
    requirements: nodeToStringArray(props[':requirements']?.value),
    acceptance: nodeToStringArray(props[':acceptance']?.value),
    report: nodeToStringArray(props[':report']?.value),
    sessionTraceWritable: keywordPropBool(props, ':session-trace-writable') === true,
    routerPolicyPath: keywordPropText(props, ':router-policy-path') ?? null,
    routerBackendRegistryPath: keywordPropText(props, ':router-backend-registry-path') ?? null,
    contextAtlasPath: keywordPropText(props, ':context-atlas-path') ?? null,
    patternCardPath: keywordPropText(props, ':pattern-card-path') ?? null,
    contextPackPath: keywordPropText(props, ':context-pack-path') ?? null,
    verificationTier: keywordPropText(props, ':verification-tier') ?? null,
    dispatchGroup: keywordPropText(props, ':dispatch-group') ?? null,
    estimatedMinutes: keywordPropText(props, ':estimated-minutes') ?? null,
    heartbeatMinutes: keywordPropText(props, ':heartbeat-minutes') ?? null,
    commit: {
      required: keywordPropBool(commitProps, ':required'),
      message: keywordPropText(commitProps, ':message'),
      scopeCheck: keywordPropText(commitProps, ':scope-check'),
    },
    sourcePath: file,
  };
}

// Derive the wave-id prefix (e.g. "wave19") from a task id like
// "wave19-05-renderer-dispatch-brief-v1". Returns null when the id does not
// follow the wave prefix convention so callers can skip wave-scoped rendering.
function deriveWaveId(taskId) {
  if (typeof taskId !== 'string') return null;
  const match = taskId.match(/^(wave\d+)/);
  return match ? match[1] : null;
}

// Resolve the shared-memory ledger path for a task by its wave id, returning
// null when the ledger file is not present on disk. The path itself is fixed
// by the shared-memory-v1 contract (.missiond/tasks/<wave>/shared-memory.lisp).
function resolveSharedMemoryPath(taskId) {
  const wave = deriveWaveId(taskId);
  if (!wave) return null;
  const rel = path.join('.missiond', 'tasks', wave, 'shared-memory.lisp');
  const abs = path.resolve(process.cwd(), rel);
  return fs.existsSync(abs) ? rel : null;
}

// Resolve the expected report-contract output path for a task. The report file
// itself usually does not exist yet at dispatch time, so this function returns
// the convention-derived path unconditionally when a wave id can be derived.
function resolveReportContractPath(taskId) {
  const wave = deriveWaveId(taskId);
  if (!wave) return null;
  return path.join('.missiond', 'tasks', wave, 'reports', `${taskId}.report.lisp`);
}

// wave23-02: auto-detect a sibling session-trace ledger in the same wave dir.
// The renderer surfaces it without requiring a contract field — its presence
// on disk is enough. The contract's :session-trace-writable flag (default
// false) decides whether the rendered brief permits the worker to APPEND
// trace events; otherwise the worker is told to read-only.
function resolveSessionTracePath(taskId) {
  const wave = deriveWaveId(taskId);
  if (!wave) return null;
  const rel = path.join('.missiond', 'tasks', wave, 'session-trace.lisp');
  const abs = path.resolve(process.cwd(), rel);
  return fs.existsSync(abs) ? rel : null;
}

// wave24-05: resolve the router-policy file for the rendered "Router Policy
// (advisory)" section. Precedence: an explicit :router-policy-path on the
// task contract wins; otherwise the renderer auto-detects the wave24-01 seed
// at .missiond/router/router-policy-v1.lisp. Returns null when neither path
// resolves on disk so the section is omitted. The renderer never executes
// the router-policy file — it only points at the path so human readers /
// ClaudeCode workers can consult the dry-run policy themselves.
function resolveRouterPolicyPath(task) {
  const candidates = [];
  if (task.routerPolicyPath) candidates.push(task.routerPolicyPath);
  candidates.push(path.join('.missiond', 'router', 'router-policy-v1.lisp'));
  for (const rel of candidates) {
    const abs = path.resolve(process.cwd(), rel);
    if (fs.existsSync(abs)) return rel;
  }
  return null;
}

// wave26-05: resolve the router-backend readiness registry file for the
// rendered "Router Policy (advisory)" section's backend-readiness context.
// Precedence mirrors resolveRouterPolicyPath: explicit
// :router-backend-registry-path on the task contract wins; otherwise the
// renderer auto-detects the wave26-01 seed at
// .missiond/router/router-backend-registry-v1.lisp. Returns null when neither
// path resolves on disk so the registry context is omitted (the rest of the
// section continues to render unchanged). The renderer never executes the
// registry file — it only points at the path so human readers / ClaudeCode
// workers can consult readiness themselves.
function resolveRouterBackendRegistryPath(task) {
  const candidates = [];
  if (task.routerBackendRegistryPath) candidates.push(task.routerBackendRegistryPath);
  candidates.push(path.join('.missiond', 'router', 'router-backend-registry-v1.lisp'));
  for (const rel of candidates) {
    const abs = path.resolve(process.cwd(), rel);
    if (fs.existsSync(abs)) return rel;
  }
  return null;
}

export function renderTask(task, sourcePath, options = {}) {
  if (options.briefMode === 'thin') return renderThinTask(task, sourcePath, options);
  const relSource = path.relative(process.cwd(), sourcePath);
  const sharedMemoryPath = resolveSharedMemoryPath(task.id);
  const reportContractPath = resolveReportContractPath(task.id);
  const sessionTracePath = resolveSessionTracePath(task.id);
  const routerPolicyPath = resolveRouterPolicyPath(task);
  const routerBackendRegistryPath = resolveRouterBackendRegistryPath(task);
  const lines = [];
  lines.push(`# ${task.id} — ${task.title}`);
  lines.push('');
  lines.push('> Generated from MissionD task-contract v1.');
  lines.push(`> Source: \`${relSource}\``);
  lines.push('');
  lines.push('## Machine Contract');
  lines.push('');
  lines.push(`- kind: \`${task.kind}\``);
  lines.push(`- status: \`${task.status}\``);
  lines.push(`- owner: \`${task.owner}\``);
  if (task.dispatchStrategy) lines.push(`- dispatch_strategy: \`${task.dispatchStrategy}\``);
  if (task.verificationTier) lines.push(`- verification_tier: \`${task.verificationTier}\``);
  if (task.dispatchGroup) lines.push(`- dispatch_group: \`${task.dispatchGroup}\``);
  if (task.estimatedMinutes) lines.push(`- estimated_minutes: \`${task.estimatedMinutes}\``);
  if (task.heartbeatMinutes) lines.push(`- heartbeat_minutes: \`${task.heartbeatMinutes}\``);
  if (task.dependsOn.length > 0) lines.push(`- depends_on: ${task.dependsOn.map(code).join(', ')}`);
  if (sharedMemoryPath) lines.push(`- shared_memory: \`${sharedMemoryPath}\``);
  if (reportContractPath) lines.push(`- report_contract: \`${reportContractPath}\``);
  if (sessionTracePath) {
    lines.push(`- session_trace: \`${sessionTracePath}\``);
    lines.push(`- session_trace_writable: \`${task.sessionTraceWritable ? 'true' : 'false'}\``);
  }
  if (task.contextAtlasPath) lines.push(`- context_atlas: \`${task.contextAtlasPath}\``);
  if (task.patternCardPath) lines.push(`- pattern_card: \`${task.patternCardPath}\``);
  if (task.contextPackPath) lines.push(`- context_pack: \`${task.contextPackPath}\``);
  lines.push('');
  if (task.dispatchStrategy === 'agent-team') {
    lines.push('## Dispatch Note');
    lines.push('');
    lines.push('使用 agent-team提高效率');
    lines.push('');
  }
  renderNavigationContext(lines, task);
  lines.push('## Goal');
  lines.push('');
  lines.push(task.goal || '(no goal supplied)');
  lines.push('');
  renderList(lines, 'Ownership', task.writeScope, 'Expected files');
  renderList(lines, 'Must Not Touch', task.mustNotTouch, 'Forbidden files');
  renderNumbered(lines, 'Requirements', task.requirements);
  renderCommands(lines, 'Acceptance Commands', task.acceptance);
  if (sharedMemoryPath) renderSharedMemory(lines, sharedMemoryPath);
  if (reportContractPath) renderReportContract(lines, reportContractPath);
  if (sessionTracePath) renderSessionTrace(lines, task, sessionTracePath);
  if (routerPolicyPath) renderRouterPolicy(lines, task, routerPolicyPath, relSource, routerBackendRegistryPath);
  lines.push('## Commit');
  lines.push('');
  if (task.commit.required) {
    lines.push('After acceptance, commit only files inside the declared write scope.');
    lines.push('');
    renderHooksDoctorPreflight(lines);
    lines.push('Stage just the declared scope, run the pre-commit scoped-index guard, then commit:');
    lines.push('');
    lines.push('```bash');
    lines.push(renderGitAdd(task.writeScope));
    lines.push(`node scripts/task-scope-guard.mjs --task ${relSource} --mode staged`);
    lines.push(`MISSIOND_TASK_CONTRACT=${relSource} \\`);
    lines.push(`  git commit -m ${JSON.stringify(task.commit.message ?? '')}`);
    lines.push('```');
    lines.push('');
    lines.push(`Scope check: \`${task.commit.scopeCheck ?? 'write-scope-only'}\`.`);
    lines.push('');
    lines.push(
      'The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).',
    );
    lines.push('');
    renderVerifyTaskContract(lines, relSource);
  } else {
    lines.push('No commit required by contract.');
    lines.push('');
  }
  renderList(lines, 'Report', task.report.length ? task.report : [
    'Commit hash or no-commit reason.',
    'Files changed.',
    'Acceptance command results.',
  ], 'Return');
  return `${lines.join('\n')}\n`;
}

function renderThinTask(task, sourcePath, options = {}) {
  const relSource = path.relative(process.cwd(), sourcePath);
  const sharedMemoryPath = resolveSharedMemoryPath(task.id);
  const reportContractPath = resolveReportContractPath(task.id);
  const sessionTracePath = resolveSessionTracePath(task.id);
  const routerPolicyPath = resolveRouterPolicyPath(task);
  const routerBackendRegistryPath = resolveRouterBackendRegistryPath(task);
  const sharedPreamble = options.sharedPreamble ?? deriveSharedPreamblePath(task.id);
  const lines = [];

  lines.push(`# ${task.id} — ${task.title}`);
  lines.push('');
  lines.push('> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.');
  lines.push(`> Source: \`${relSource}\``);
  if (sharedPreamble) lines.push(`> Shared preamble: \`${sharedPreamble}\``);
  lines.push('');
  lines.push('## Task Contract');
  lines.push('');
  lines.push(`- kind: \`${task.kind}\``);
  lines.push(`- owner: \`${task.owner}\``);
  if (task.dispatchStrategy) lines.push(`- dispatch_strategy: \`${task.dispatchStrategy}\``);
  if (task.verificationTier) lines.push(`- verification_tier: \`${task.verificationTier}\``);
  if (task.dispatchGroup) lines.push(`- dispatch_group: \`${task.dispatchGroup}\``);
  if (task.estimatedMinutes) lines.push(`- estimated_minutes: \`${task.estimatedMinutes}\``);
  if (task.heartbeatMinutes) lines.push(`- heartbeat_minutes: \`${task.heartbeatMinutes}\``);
  if (task.dependsOn.length > 0) lines.push(`- depends_on: ${task.dependsOn.map(code).join(', ')}`);
  if (sharedMemoryPath) lines.push(`- shared_memory: \`${sharedMemoryPath}\``);
  if (reportContractPath) lines.push(`- report_contract: \`${reportContractPath}\``);
  if (sessionTracePath) lines.push(`- session_trace: \`${sessionTracePath}\` (${task.sessionTraceWritable ? 'writable' : 'read-only'})`);
  if (routerPolicyPath) lines.push(`- router_policy: \`${routerPolicyPath}\` (advisory / dry-run only)`);
  if (routerBackendRegistryPath) lines.push(`- router_backend_registry: \`${routerBackendRegistryPath}\` (MUST NOT switch backend)`);
  if (task.contextAtlasPath) lines.push(`- context_atlas: \`${task.contextAtlasPath}\``);
  if (task.patternCardPath) lines.push(`- pattern_card: \`${task.patternCardPath}\``);
  if (task.contextPackPath) lines.push(`- context_pack: \`${task.contextPackPath}\``);
  lines.push('');
  renderNavigationContext(lines, task);
  lines.push('## Goal');
  lines.push('');
  lines.push(task.goal || '(no goal supplied)');
  lines.push('');
  renderList(lines, 'Ownership', task.writeScope, 'Expected files');
  renderList(lines, 'Must Not Touch', task.mustNotTouch, 'Forbidden files');
  renderNumbered(lines, 'Requirements', task.requirements);
  renderCommands(lines, 'Acceptance Commands', task.acceptance);
  lines.push('## Shared Protocol');
  lines.push('');
  if (sharedPreamble) {
    lines.push(`Read \`${sharedPreamble}\` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.`);
  } else {
    lines.push('Use the repository shared-memory, report, session-trace, hook, commit, and verifier protocol for this wave.');
  }
  lines.push('- Task-specific scope and acceptance above override generic guidance.');
  if (task.contextAtlasPath || task.patternCardPath || task.contextPackPath) {
    lines.push('- Load declared context surfaces before broad repository search; use atlas/card anchors and the latest context-pack integration-plan to reduce navigation misses.');
  }
  lines.push('- Append coordination facts to shared memory when present; write the report contract when the task completes.');
  if (task.heartbeatMinutes) {
    lines.push(`- If work is still active after ${task.heartbeatMinutes} minutes without a completion, append a heartbeat/observation entry or report a blocker.`);
  }
  lines.push('');
  lines.push('## Commit');
  lines.push('');
  if (task.commit.required) {
    lines.push('Commit only files inside the declared write scope after acceptance:');
    lines.push('');
    lines.push('```bash');
    lines.push(renderGitAdd(task.writeScope));
    lines.push(`node scripts/task-scope-guard.mjs --task ${relSource} --mode staged`);
    lines.push(`MISSIOND_TASK_CONTRACT=${relSource} \\`);
    lines.push(`  git commit -m ${JSON.stringify(task.commit.message ?? '')}`);
    lines.push(`node scripts/verify-task-contract.mjs ${relSource}`);
    lines.push('```');
    lines.push('');
  } else {
    lines.push('No commit required by contract.');
    lines.push('');
  }
  renderList(lines, 'Report', task.report.length ? task.report : [
    'Commit hash or no-commit reason.',
    'Files changed.',
    'Acceptance command results.',
  ], 'Return');
  return `${lines.join('\n')}\n`;
}

function renderNavigationContext(lines, task) {
  if (!task.contextAtlasPath && !task.patternCardPath && !task.contextPackPath) return;
  lines.push('## Context Navigation');
  lines.push('');
  if (task.contextAtlasPath) {
    lines.push(`- Read context atlas first: \`${task.contextAtlasPath}\`.`);
  }
  if (task.patternCardPath) {
    lines.push(`- Follow implementation pattern card: \`${task.patternCardPath}\`.`);
  }
  if (task.contextPackPath) {
    lines.push(`- Read context-pack integration-plan before implementation: \`${task.contextPackPath}\`.`);
    lines.push('- Treat accepted shards and mapped dispatch groups as the shard authority; do not re-derive architecture from older observations.');
  }
  lines.push('- Use declared context anchors and shard boundaries before falling back to broad scans.');
  lines.push('');
}

export function renderSharedPreamble() {
  const lines = [];
  lines.push('# MissionD ClaudeCode Shared Preamble');
  lines.push('');
  lines.push('This file carries common execution protocol for thin task briefs. The task Lisp is still the source of truth for scope, dependencies, acceptance, and commit policy.');
  lines.push('');
  lines.push('## Execution Rules');
  lines.push('');
  lines.push('- Treat the task contract as the machine SSOT; Markdown is a human execution view.');
  lines.push('- Stay inside `:write-scope`; never edit paths matching `:must-not-touch`.');
  lines.push('- Use focused read/search tools first. Use shell mainly for deterministic checks, tests, and git inspection.');
  lines.push('- Prefer local/smoke verification while implementing; leave full workspace builds for smoke/final tasks unless the task explicitly requires them.');
  lines.push('');
  lines.push('## Shared Memory');
  lines.push('');
  lines.push('- Append `claim` before starting, `observation` or `blocker` while running, and `completion` when finished.');
  lines.push('- Entries are append-only S-expressions; never edit prior entries.');
  lines.push('- If the task has `:heartbeat-minutes`, append a heartbeat/observation before that interval elapses when still active.');
  lines.push('');
  lines.push('## Report Contract');
  lines.push('');
  lines.push('- Write the expected report under `.missiond/tasks/<wave>/reports/<task-id>.report.lisp` when the task completes.');
  lines.push('- Keep structural proof in fields; put prose explanation in `:notes` and trace references.');
  lines.push('- Run `node scripts/check-task-report.mjs <report>` before commit when a report is required.');
  lines.push('');
  lines.push('## Session Trace');
  lines.push('');
  lines.push('- Treat `session-trace.lisp` as factual telemetry, not prose notes.');
  lines.push('- When this task is trace-writable, append a read/observation event after loading the shared preamble so preamble usage is auditable.');
  lines.push('- Write trace entries only when the task contract says `:session-trace-writable true`; otherwise read it only.');
  lines.push('');
  lines.push('## Router Context');
  lines.push('');
  lines.push('- Router policy, recommendation, readiness, and dispatch descriptor outputs are advisory and dry-run only.');
  lines.push('- `runtime_replacement=false`, `dry_run_only=true`, `no_execution=true`, and `applied=false` remain hard boundaries.');
  lines.push('- Never switch backend based on rendered brief text or descriptor evidence.');
  lines.push('');
  lines.push('## Commit Protocol');
  lines.push('');
  lines.push('```bash');
  lines.push('node scripts/check-missiond-hooks.mjs --json');
  lines.push('node scripts/task-scope-guard.mjs --task <task.lisp> --mode staged');
  lines.push('MISSIOND_TASK_CONTRACT=<task.lisp> git commit -m "<message>"');
  lines.push('node scripts/verify-task-contract.mjs <task.lisp>');
  lines.push('```');
  lines.push('');
  return `${lines.join('\n')}\n`;
}

export function deriveSharedPreamblePath(taskId) {
  const wave = deriveWaveId(taskId);
  if (!wave) return null;
  return path.join('.missiond', 'claudecode', `${wave}-shared-preamble.md`);
}

function renderSharedMemory(lines, sharedMemoryPath) {
  lines.push('## Shared Memory');
  lines.push('');
  lines.push(`Coordination ledger: \`${sharedMemoryPath}\` (schema \`missiond.shared-memory.v1\`).`);
  lines.push('');
  lines.push('- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.');
  lines.push('- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.');
  lines.push('- `:touched` paths in your entries must stay inside this task `:write-scope`.');
  lines.push('');
  lines.push('Validate with:');
  lines.push('');
  lines.push('```bash');
  lines.push(`node scripts/check-task-memory.mjs ${sharedMemoryPath}`);
  lines.push('```');
  lines.push('');
}

function renderReportContract(lines, reportContractPath) {
  lines.push('## Report Contract');
  lines.push('');
  lines.push(`Expected machine-readable report: \`${reportContractPath}\` (schema \`missiond.report-contract.v1\`).`);
  lines.push('');
  lines.push('- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.');
  lines.push('- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.');
  lines.push('- Free-form prose belongs in `:notes`; structural fields drive automated verification.');
  lines.push('- Optional worker-explanation fields (prose only — facts live in `session-trace.lisp`):');
  lines.push('  - `:time_sinks` — vector of strings or `(:label <s> [:duration_ms <int>] [:notes <s>])` entries.');
  lines.push('  - `:major_decisions` — vector of strings or `(:decision <s> [:rationale <s>] [:trace_ref <s>])` entries.');
  lines.push('  - `:unexpected_work` — vector of strings or `(:summary <s> [:trace_ref <s>])` entries.');
  lines.push('  - `:blockers` — vector of strings or `(:summary <s> [:resolved <bool>] [:trace_ref <s>])` entries.');
  lines.push('  - `:trace_refs` — vector of session-trace event ids or repo-relative paths linking back to factual telemetry.');
  lines.push('- Optional router-recommendation fields (wave25-02 — populate ONLY when you observe a dry-run recommendation; the recommendation is **advisory** and **dry-run only**, never authoritative for dispatch):');
  lines.push('  - `:recommended_backend` — string enum: `claudecode | missiond-llm-router | deterministic-checker | patch-worker | verifier-worker`.');
  lines.push('  - `:router_confidence` — string enum: `high | medium | low`.');
  lines.push('  - `:router_policy_path` — repo-relative path to the policy consulted.');
  lines.push('  - `:router_dry_run_only` — literal `true` (cross-wave invariant).');
  lines.push('  - `:router_applied` — literal `false` (cross-wave invariant — runtime replacement is rejected).');
  lines.push('  - `:router_reasons` — vector of non-empty strings.');
  lines.push('  - `:router_trace_index_path` — repo-relative path to the trace index that scored confidence (when used).');
  lines.push('- Optional router-readiness fields (wave26-04 — populate ONLY when you observe a backend readiness registry; readiness is **advisory** and **dry-run only**, and you MUST NOT switch backend based on these values):');
  lines.push('  - `:router_backend_readiness_status` — string enum: `current-default | advisory-only | runtime-ready | unavailable | unknown`.');
  lines.push('  - `:router_backend_runtime_allowed` — literal `true` or `false` (atom, never a string).');
  lines.push('  - `:router_apply_eligible` — literal `true` or `false` (atom, never a string; current-default alone is NEVER sufficient — explicit runtime-ready opt-in required upstream).');
  lines.push('  - `:router_apply_blockers` — vector of non-empty strings (no property-list entries).');
  lines.push('  - `:router_backend_registry_path` — repo-relative path to the backend readiness registry consulted.');
  lines.push('- Optional router-dispatch-descriptor fields (wave27-04 — populate ONLY when you observe a dispatch descriptor produced by `scripts/build-router-dispatch-descriptor.mjs`; descriptors are **advisory**, **dry-run only**, and carry **no execution** semantics, and you MUST NOT switch backend based on these values):');
  lines.push('  - `:router_dispatch_descriptor_path` — repo-relative path to the dispatch descriptor consulted.');
  lines.push('  - `:router_dispatch_descriptor_status` — string enum: `eligible | current-default | advisory-only | registry-missing | unavailable | unknown`.');
  lines.push('  - `:router_dispatch_backend` — string enum: `claudecode | missiond-llm-router | deterministic-checker | patch-worker | verifier-worker`.');
  lines.push('  - `:router_dispatch_eligible` — literal `true` or `false` (atom, never a string).');
  lines.push('  - `:router_dispatch_no_execution` — literal `true` ONLY (cross-wave invariant; literal `false` AND any quoted-string form are rejected by the checker).');
  lines.push('  - `:router_dispatch_blockers` — vector of non-empty strings (no property-list entries).');
  lines.push('');
  lines.push('Validate with:');
  lines.push('');
  lines.push('```bash');
  lines.push(`node scripts/check-task-report.mjs ${reportContractPath}`);
  lines.push('```');
  lines.push('');
}

function renderSessionTrace(lines, task, sessionTracePath) {
  lines.push('## Session Trace');
  lines.push('');
  lines.push(`Factual telemetry ledger: \`${sessionTracePath}\` (schema \`missiond.session-trace.v1\`).`);
  lines.push('');
  lines.push('- This file is the single source of truth for what happened: dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events.');
  lines.push('- Worker prose explanations belong in the report contract\'s `:time_sinks` / `:major_decisions` / `:unexpected_work` / `:blockers` / `:trace_refs` fields, not here.');
  if (task.sessionTraceWritable) {
    lines.push('- This task is `:session-trace-writable true`: you MAY append `(trace-event ...)` entries to the ledger as factual coordination output, in addition to your declared `:write-scope`. Entries must follow the schema (required `:id` `:seq` `:at` `:task` `:backend` `:kind` `:summary`).');
    lines.push('- Treat the trace ledger as an append-only journal: never edit prior events; record corrections as new events that reference the prior `:id` via `:trace_refs`.');
  } else {
    lines.push('- This task is **not** `:session-trace-writable` (default). You MUST NOT write to `session-trace.lisp` — read it for context only. Telemetry for this task is recorded by MissionD or by tasks explicitly opted in via `:session-trace-writable true`.');
  }
  lines.push('');
  lines.push('Validate the ledger after any change with:');
  lines.push('');
  lines.push('```bash');
  lines.push(`node scripts/check-session-trace.mjs ${sessionTracePath}`);
  lines.push('```');
  lines.push('');
}

// wave24-05: emit a "Router Policy (advisory)" section between Session Trace
// and Commit when a router-policy file resolves on disk (either via the
// task contract's :router-policy-path or the auto-detected wave24-01 seed
// at .missiond/router/router-policy-v1.lisp). The section is informational
// only — it MUST contain the literal words "advisory" and "dry-run only" to
// reinforce that runtime dispatch is unchanged, and it MUST NOT instruct
// ClaudeCode (or any worker) to switch backend. The renderer never invokes
// scripts/recommend-task-backend.mjs; recommendation remains an opt-in CLI
// for humans and tooling, not a hidden side-effect of rendering a brief.
//
// wave26-05: the same section is extended to surface the wave26-01 backend
// readiness registry when a registry path resolves (either via the task
// contract's :router-backend-registry-path or the auto-detected
// .missiond/router/router-backend-registry-v1.lisp seed). When the registry
// resolves, a read-only `node scripts/check-router-backend-registry.mjs
// <registry-path>` line is appended; when BOTH the policy AND the registry
// resolve, the wave25-04 recommend-task-backend command line gains
// `--backend-registry <registry-path>` (when only the policy resolves the
// wave25-04 command stays rendered byte-identical for backward compat).
// Section keeps 'advisory' / 'dry-run only' literals verbatim and explicitly
// reiterates the worker MUST NOT switch backend on the strength of rendered
// text. Renderer still never shells out — every command in the section is
// rendered text only.
//
// wave27-05: the same section is extended further so that when BOTH the
// policy AND the registry resolve, the renderer appends a dedicated
// dispatch-descriptor sub-section carrying TWO read-only command lines as
// text — `node scripts/build-router-dispatch-descriptor.mjs --task <task>
// --policy <policy> --backend-registry <registry>` (default Lisp output)
// and the same command piped into `node scripts/check-router-dispatch-
// descriptor.mjs --stdin` (the wave27-01 checker only parses Lisp on
// stdin, so the rendered pipe form intentionally drops --json). The
// sub-section adds the literal 'no execution' phrase next to the existing
// 'advisory' / 'dry-run only' / 'MUST NOT switch backend' literals.
// Descriptors are ephemeral generated artifacts so no new task-contract
// field is introduced — the descriptor is built on demand from existing
// inputs (task + policy + registry). Renderer still never shells out.
function renderRouterPolicy(lines, task, routerPolicyPath, relSource, routerBackendRegistryPath) {
  const explicitPolicy = task.routerPolicyPath && task.routerPolicyPath === routerPolicyPath;
  const explicitRegistry =
    task.routerBackendRegistryPath && task.routerBackendRegistryPath === routerBackendRegistryPath;
  lines.push('## Router Policy (advisory)');
  lines.push('');
  lines.push(`Dry-run router-policy ledger: \`${routerPolicyPath}\` (schema \`missiond.router-policy.v1\`).`);
  if (routerBackendRegistryPath) {
    lines.push(`Backend readiness registry: \`${routerBackendRegistryPath}\` (schema \`missiond.router-backend-registry.v1\`).`);
  }
  lines.push('');
  lines.push('- This section is **advisory** and **dry-run only**. The policy file is informational; it captures backend recommendations distilled from prior session-trace observations, but **runtime dispatch is unchanged** — ClaudeCode remains the live backend for this task.');
  lines.push('- The brief surfaces the policy path so human readers and ClaudeCode workers can consult the recommendations; it does not instruct the worker to switch backend, alter the dispatch strategy, or run the recommendation CLI.');
  lines.push('- **You MUST NOT switch backend** based on anything rendered in this section. The recommendation and readiness fields below are observational signals only — runtime dispatch never changes as a side-effect of reading this brief, and apply-eligibility is recorded for telemetry, not for worker action.');
  if (explicitPolicy) {
    lines.push('- Policy source: explicit `:router-policy-path` on the task contract.');
  } else {
    lines.push('- Policy source: auto-detected default seed (no `:router-policy-path` on the task contract).');
  }
  if (routerBackendRegistryPath) {
    if (explicitRegistry) {
      lines.push('- Registry source: explicit `:router-backend-registry-path` on the task contract.');
    } else {
      lines.push('- Registry source: auto-detected default seed (no `:router-backend-registry-path` on the task contract).');
    }
  }
  lines.push('');
  lines.push('Inspect the policy with the read-only checker (the renderer itself does not execute the policy or shell out to the recommendation CLI):');
  lines.push('');
  lines.push('```bash');
  lines.push(`node scripts/check-router-policy.mjs ${routerPolicyPath}`);
  if (routerBackendRegistryPath) {
    lines.push(`node scripts/check-router-backend-registry.mjs ${routerBackendRegistryPath}`);
  }
  lines.push('```');
  lines.push('');
  lines.push('You **may** also inspect the dry-run recommendation for THIS task by running the recommendation CLI directly. The renderer never executes it — the command below is rendered text only and stays **advisory** and **dry-run only**; the recommendation does not change dispatch and you MUST NOT switch backend on the strength of its output:');
  lines.push('');
  lines.push('```bash');
  if (routerBackendRegistryPath) {
    lines.push(
      `node scripts/recommend-task-backend.mjs --task ${relSource} --policy ${routerPolicyPath} --backend-registry ${routerBackendRegistryPath} --json`,
    );
  } else {
    lines.push(`node scripts/recommend-task-backend.mjs --task ${relSource} --policy ${routerPolicyPath} --json`);
  }
  lines.push('```');
  lines.push('');
  // wave27-05: dispatch-descriptor sub-section — rendered ONLY when BOTH
  // the policy AND the registry resolve, because the wave27-02 CLI
  // requires --task + --policy + --backend-registry to emit a descriptor.
  // The literal 'no execution' phrase is REQUIRED here (cross-wave
  // invariant). The pipe form deliberately drops --json because the
  // wave27-01 checker only parses Lisp on stdin (per wave27-02 finding).
  if (routerBackendRegistryPath) {
    lines.push('You **may** also build a dispatch descriptor from THIS task plus the policy and registry by running the descriptor CLI directly. The renderer never executes it — the commands below are rendered text only and stay **advisory**, **dry-run only**, and **no execution**; the descriptor never changes dispatch and you MUST NOT switch backend on the strength of its output:');
    lines.push('');
    lines.push('```bash');
    lines.push(
      `node scripts/build-router-dispatch-descriptor.mjs --task ${relSource} --policy ${routerPolicyPath} --backend-registry ${routerBackendRegistryPath}`,
    );
    lines.push(
      `node scripts/build-router-dispatch-descriptor.mjs --task ${relSource} --policy ${routerPolicyPath} --backend-registry ${routerBackendRegistryPath} | node scripts/check-router-dispatch-descriptor.mjs --stdin`,
    );
    lines.push('```');
    lines.push('');
  }
}

function renderVerifyTaskContract(lines, relSource) {
  lines.push('Verify the commit against this contract (read-only, post-commit):');
  lines.push('');
  lines.push('```bash');
  lines.push(`node scripts/verify-task-contract.mjs ${relSource}`);
  lines.push('```');
  lines.push('');
}

// Default-on hooks doctor preflight v2: surface the read-only doctor and the
// explicit (opt-in) installer command BEFORE the staged guard / commit
// commands so dispatched agents see core.hooksPath as a first-class
// preflight expectation. The renderer never mutates git config; it just
// emits the doctor command. Only `install-missiond-hooks.mjs --install` may
// flip core.hooksPath, and that is left to the operator/agent to run
// explicitly.
function renderHooksDoctorPreflight(lines) {
  lines.push('Preflight: confirm the repo-local `core.hooksPath` doctor is green so the shared `.githooks/pre-commit` hook also enforces the staged guard. Drift here is a preflight problem, not a hard error — the doctor is read-only; only `--install` mutates git config.');
  lines.push('');
  lines.push('```bash');
  lines.push('node scripts/check-missiond-hooks.mjs --json   # read-only doctor; reports preflight-drift on unset/wrong path');
  lines.push('node scripts/install-missiond-hooks.mjs --install   # only run when the doctor reports drift; writes --local config only');
  lines.push('```');
  lines.push('');
}

function renderList(lines, title, items, label) {
  lines.push(`## ${title}`);
  lines.push('');
  if (!items.length) {
    lines.push(`- (${label.toLowerCase()} empty)`);
  } else {
    for (const item of items) lines.push(`- \`${item}\``);
  }
  lines.push('');
}

function renderNumbered(lines, title, items) {
  lines.push(`## ${title}`);
  lines.push('');
  if (!items.length) {
    lines.push('No additional requirements.');
  } else {
    for (let i = 0; i < items.length; i++) lines.push(`${i + 1}. ${items[i]}`);
  }
  lines.push('');
}

function renderCommands(lines, title, commands) {
  lines.push(`## ${title}`);
  lines.push('');
  lines.push('```bash');
  for (const command of commands) lines.push(command);
  lines.push('```');
  lines.push('');
}

function renderGitAdd(paths) {
  if (paths.length === 0) return '# no write scope declared';
  return `git add ${paths.map((p) => JSON.stringify(p)).join(' \\\n        ')}`;
}

function code(value) {
  return `\`${value}\``;
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

// wave26-06 Layer D cross-layer smoke: render a synthetic in-memory task
// that opts into BOTH `:router-policy-path` and `:router-backend-registry-
// path`, then assert the rendered Markdown carries every wave24-05 +
// wave25-04 + wave26-05 literal that downstream coordination depends on.
// Pure-in-process: no shell-out, no LLM probe, no git, no network. The
// fixture writes its synthetic task to the OS tmp dir so loadSingleTask
// (which reads from disk) can drive the same code path the production
// renderer uses; the tmp file is removed on exit.
function runFixtures() {
  const fixtures = [];
  fixtures.push({
    name: 'wave26-06-renderer-readiness-literals: rendered brief carries advisory + dry-run only + router-backend-registry + --backend-registry + MUST NOT switch backend',
    category: 'wave26-06-renderer-readiness-literals',
    run: () => {
      // Synthetic task contract that opts into BOTH router fields. The
      // task body mirrors a wave26-style smoke contract minus the
      // wave-specific fields the smoke does not need (wave26-05 schema
      // additions plus the wave24-05 :router-policy-path).
      const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wave26-06-render-'));
      const tmpTask = path.join(tmpDir, 'wave26-06-render-fixture.lisp');
      const lisp = `(task wave26-06-render-fixture\n  :schema "missiond.task-contract.v1"\n  :title "Render fixture"\n  :kind smoke\n  :status ready\n  :owner "claudecode"\n  :dispatch-strategy "manual"\n  :goal "Render fixture goal"\n  :write-scope ["scripts/x.mjs"]\n  :must-not-touch []\n  :requirements []\n  :acceptance ["true"]\n  :router-policy-path ".missiond/router/router-policy-v1.lisp"\n  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"\n  :commit (:required false :scope-check none)\n  :report ["Commit hash."])\n`;
      try {
        fs.writeFileSync(tmpTask, lisp, 'utf8');
        // Drive the same loadSingleTask + renderTask path production
        // uses. We need both router files to resolve so the renderer
        // emits the wave26-05 surface; the auto-detect default seeds
        // are checked against the repo's actual files (the renderer
        // resolves them relative to process.cwd()).
        const task = loadSingleTask(tmpTask);
        const markdown = renderTask(task, tmpTask);
        // Required literals — every entry is a substring search; if any
        // literal goes missing the smoke fails loudly with the missing
        // token name. The `--backend-registry` and `MUST NOT switch
        // backend` literals are wave26-05 additions; the others
        // pre-date wave26-05 (wave24-05 / wave25-04).
        const required = [
          'advisory',
          'dry-run only',
          'router-backend-registry',
          '--backend-registry',
          'MUST NOT switch backend',
        ];
        for (const literal of required) {
          if (!markdown.includes(literal)) {
            throw new Error(
              `wave26-06 invariant: rendered brief missing required literal '${literal}'`,
            );
          }
        }
        // Cross-wave invariant: rendered brief surfaces both router file
        // paths verbatim so downstream tooling can grep them.
        if (!markdown.includes('.missiond/router/router-policy-v1.lisp')) {
          throw new Error('wave26-06 invariant: rendered brief missing router-policy path');
        }
        if (!markdown.includes('.missiond/router/router-backend-registry-v1.lisp')) {
          throw new Error('wave26-06 invariant: rendered brief missing router-backend-registry path');
        }
        // Cross-wave invariant: the recommend-task-backend command
        // includes the --backend-registry flag when BOTH files resolve
        // (wave26-05 contract). A regression that drops the flag fails
        // here.
        if (!markdown.match(/recommend-task-backend\.mjs.*--backend-registry/)) {
          throw new Error(
            'wave26-06 invariant: recommend-task-backend command must include --backend-registry when registry resolves',
          );
        }
      } finally {
        fs.rmSync(tmpDir, { recursive: true, force: true });
      }
    },
  });
  // wave27-05 dispatch-descriptor literals: rendered brief carries the
  // full wave27-05 surface — `advisory` + `dry-run only` + `no execution`
  // + `MUST NOT switch backend` + `build-router-dispatch-descriptor` —
  // when BOTH the policy AND the registry resolve. Reuses the same
  // synthetic on-disk task contract pattern as wave26-06 so the same
  // loadSingleTask + renderTask code path drives the render.
  fixtures.push({
    name: 'wave27-05-renderer-dispatch-descriptor-literals: rendered brief carries advisory + dry-run only + no execution + MUST NOT switch backend + build-router-dispatch-descriptor',
    category: 'wave27-05-renderer-dispatch-descriptor-literals',
    run: () => {
      const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wave27-05-render-'));
      const tmpTask = path.join(tmpDir, 'wave27-05-render-fixture.lisp');
      const lisp = `(task wave27-05-render-fixture\n  :schema "missiond.task-contract.v1"\n  :title "Render fixture"\n  :kind smoke\n  :status ready\n  :owner "claudecode"\n  :dispatch-strategy "manual"\n  :goal "Render fixture goal"\n  :write-scope ["scripts/x.mjs"]\n  :must-not-touch []\n  :requirements []\n  :acceptance ["true"]\n  :router-policy-path ".missiond/router/router-policy-v1.lisp"\n  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"\n  :commit (:required false :scope-check none)\n  :report ["Commit hash."])\n`;
      try {
        fs.writeFileSync(tmpTask, lisp, 'utf8');
        const task = loadSingleTask(tmpTask);
        const markdown = renderTask(task, tmpTask);
        // Required literals — substring search; if any goes missing the
        // smoke fails loudly with the missing token name. The
        // `no execution` and `build-router-dispatch-descriptor` literals
        // are wave27-05 additions; the others pre-date wave27-05
        // (wave24-05 / wave25-04 / wave26-05) and are re-asserted to
        // catch regressions.
        const required = [
          'advisory',
          'dry-run only',
          'no execution',
          'MUST NOT switch backend',
          'build-router-dispatch-descriptor',
        ];
        for (const literal of required) {
          if (!markdown.includes(literal)) {
            throw new Error(
              `wave27-05 invariant: rendered brief missing required literal '${literal}'`,
            );
          }
        }
        // Cross-wave invariant: the dispatch-descriptor sub-section
        // renders BOTH the default Lisp form AND the pipe-to-checker
        // form. The pipe form deliberately drops --json (wave27-01
        // checker only parses Lisp on stdin per wave27-02 finding).
        if (!markdown.includes('| node scripts/check-router-dispatch-descriptor.mjs --stdin')) {
          throw new Error(
            'wave27-05 invariant: rendered brief missing the pipe-to-check-router-dispatch-descriptor.mjs --stdin form',
          );
        }
        if (markdown.match(/build-router-dispatch-descriptor\.mjs[^\n|]*--json[^\n]*\| node scripts\/check-router-dispatch-descriptor/)) {
          throw new Error(
            'wave27-05 invariant: pipe form must NOT include --json (wave27-01 checker parses Lisp from stdin only)',
          );
        }
        // Cross-wave invariant: the wave26-05 recommend-task-backend
        // command line MUST still carry --backend-registry when both
        // files resolve (regression guard for wave26-05 surface).
        if (!markdown.match(/recommend-task-backend\.mjs.*--backend-registry/)) {
          throw new Error(
            'wave27-05 regression: recommend-task-backend command must still include --backend-registry when registry resolves',
          );
        }
        // Cross-wave invariant: report-contract section enumerates all
        // 6 wave27-04 dispatch-descriptor fields with MAY-language.
        const reportFields = [
          ':router_dispatch_descriptor_path',
          ':router_dispatch_descriptor_status',
          ':router_dispatch_backend',
          ':router_dispatch_eligible',
          ':router_dispatch_no_execution',
          ':router_dispatch_blockers',
        ];
        for (const field of reportFields) {
          if (!markdown.includes(field)) {
            throw new Error(
              `wave27-05 invariant: Report Contract section missing wave27-04 field '${field}'`,
            );
          }
        }
      } finally {
        fs.rmSync(tmpDir, { recursive: true, force: true });
      }
    },
  });
  // Static-audit smoke: the renderer module itself must remain free of
  // shell-out / LLM clients / network / mutating git invocations. The
  // forbidden-pattern table is assembled from string parts so this
  // audit does not self-trip on the patterns it scans for (wave24-06 /
  // wave25-01 / wave25-05 self-audit lesson).
  fixtures.push({
    name: 'wave26-06-renderer-static-audit: zero shell/LLM/git/network in renderer',
    category: 'wave26-06-renderer-static-audit',
    run: () => {
      const selfPath = path.resolve(process.cwd(), 'scripts', 'render-claudecode-task.mjs');
      const src = fs.readFileSync(selfPath, 'utf8');
      const stripped = src
        .split('\n')
        .filter((ln) => !/^\s*\/\//.test(ln))
        .join('\n')
        .replace(/\/\*[\s\S]*?\*\//g, '')
        .replace(/'(?:\\.|[^'\\\n])*'/g, "''")
        .replace(/"(?:\\.|[^"\\\n])*"/g, '""')
        .replace(/`(?:\\.|[^`\\])*`/g, '``');
      const forbidden = [
        new RegExp('child' + '_' + 'process'),
        new RegExp('\\bspawn\\('),
        new RegExp('\\bspawn' + 'Sync\\b'),
        new RegExp('\\bexec' + 'Sync\\b'),
        new RegExp('\\bfork\\('),
        new RegExp('open' + 'ai', 'i'),
        new RegExp('anthrop' + 'ic', 'i'),
        new RegExp('chat\\.compl' + 'etion', 'i'),
        new RegExp('\\bfetch\\('),
        new RegExp('\\bhttps?\\.(?:get|request|post)\\b'),
      ];
      for (const re of forbidden) {
        if (re.test(stripped)) {
          throw new Error(
            `wave26-06 self-audit: forbidden pattern ${re} found in renderer active source`,
          );
        }
      }
    },
  });
  // wave27-05 static-audit: re-assert that the renderer module remains
  // free of shell-out / LLM / network / mutating git after the wave27-05
  // additions. Pattern table is assembled from string parts so the
  // audit does not self-trip on the patterns it scans for (wave24-06 /
  // wave25-05 / wave26-06 lesson). Adds extra `git ` mutating-verb
  // patterns so the wave27-05 surface is also covered against any
  // future hidden git invocation.
  fixtures.push({
    name: 'wave27-05-renderer-static-audit: zero shell/LLM/git/network in renderer post wave27-05',
    category: 'wave27-05-renderer-static-audit',
    run: () => {
      const selfPath = path.resolve(process.cwd(), 'scripts', 'render-claudecode-task.mjs');
      const src = fs.readFileSync(selfPath, 'utf8');
      const stripped = src
        .split('\n')
        .filter((ln) => !/^\s*\/\//.test(ln))
        .join('\n')
        .replace(/\/\*[\s\S]*?\*\//g, '')
        .replace(/'(?:\\.|[^'\\\n])*'/g, "''")
        .replace(/"(?:\\.|[^"\\\n])*"/g, '""')
        .replace(/`(?:\\.|[^`\\])*`/g, '``');
      const forbidden = [
        new RegExp('child' + '_' + 'process'),
        new RegExp('\\bspawn\\('),
        new RegExp('\\bspawn' + 'Sync\\b'),
        new RegExp('\\bexec' + 'Sync\\b'),
        new RegExp('\\bexec' + 'File\\b'),
        new RegExp('\\bfork\\('),
        new RegExp('open' + 'ai', 'i'),
        new RegExp('anthrop' + 'ic', 'i'),
        new RegExp('chat\\.compl' + 'etion', 'i'),
        new RegExp('\\bfetch\\('),
        new RegExp('\\bhttps?\\.(?:get|request|post)\\b'),
        new RegExp('\\bnet\\.create' + 'Connection\\b'),
        new RegExp('simple' + 'Git\\('),
      ];
      for (const re of forbidden) {
        if (re.test(stripped)) {
          throw new Error(
            `wave27-05 self-audit: forbidden pattern ${re} found in renderer active source`,
          );
        }
      }
    },
  });
  // wave27-06 cross-layer smoke: pin the FULL set of literals the
  // wave27 renderer surface MUST emit when both router-policy AND
  // backend-registry resolve. Same loadSingleTask + renderTask code
  // path as wave26-06 / wave27-05; the wave27-06 distinction is
  // exhaustive: 6 patterns asserted in one fixture so a future
  // regression on ANY of them surfaces immediately under a
  // wave27-06 grep. Patterns: 'advisory' / 'dry-run only' /
  // 'no execution' / 'MUST NOT switch backend' /
  // 'build-router-dispatch-descriptor' /
  // 'check-router-dispatch-descriptor'.
  fixtures.push({
    name: 'wave27-06-renderer-literals: rendered brief carries advisory + dry-run only + no execution + MUST NOT switch backend + build-router-dispatch-descriptor + check-router-dispatch-descriptor',
    category: 'wave27-06-renderer-literals',
    run: () => {
      const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wave27-06-render-'));
      const tmpTask = path.join(tmpDir, 'wave27-06-render-fixture.lisp');
      const lisp = `(task wave27-06-render-fixture\n  :schema "missiond.task-contract.v1"\n  :title "Render fixture"\n  :kind smoke\n  :status ready\n  :owner "claudecode"\n  :dispatch-strategy "manual"\n  :goal "Render fixture goal"\n  :write-scope ["scripts/x.mjs"]\n  :must-not-touch []\n  :requirements []\n  :acceptance ["true"]\n  :router-policy-path ".missiond/router/router-policy-v1.lisp"\n  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"\n  :commit (:required false :scope-check none)\n  :report ["Commit hash."])\n`;
      try {
        fs.writeFileSync(tmpTask, lisp, 'utf8');
        const task = loadSingleTask(tmpTask);
        const markdown = renderTask(task, tmpTask);
        // The wave27-06 invariant — every literal listed below MUST be
        // present in the rendered brief. If any is missing the smoke
        // fails loudly with the missing token name so a regression is
        // attributable to the exact layer that dropped it.
        const required = [
          'advisory',
          'dry-run only',
          'no execution',
          'MUST NOT switch backend',
          'build-router-dispatch-descriptor',
          'check-router-dispatch-descriptor',
        ];
        for (const literal of required) {
          if (!markdown.includes(literal)) {
            throw new Error(
              `wave27-06 invariant: rendered brief missing required literal '${literal}'`,
            );
          }
        }
      } finally {
        fs.rmSync(tmpDir, { recursive: true, force: true });
      }
    },
  });
  // wave28 dispatch-efficiency fixture: thin briefs must point at a
  // shared preamble and omit the verbose boilerplate sections that used
  // to be repeated in every worker prompt. Full mode remains the default
  // path, so this fixture exercises the explicit --brief-mode thin code.
  fixtures.push({
    name: 'wave28-thin-brief: shared preamble pointer plus no repeated boilerplate sections',
    category: 'wave28-dispatch-efficiency',
    run: () => {
      const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wave28-thin-render-'));
      const tmpTask = path.join(tmpDir, 'wave28-01-thin-render-fixture.lisp');
      const lisp = `(task wave28-01-thin-render-fixture\n  :schema "missiond.task-contract.v1"\n  :title "Thin render fixture"\n  :kind code-alignment\n  :status ready\n  :owner "claudecode"\n  :dispatch-strategy "fresh-code-alignment"\n  :verification-tier smoke\n  :dispatch-group "A"\n  :estimated-minutes 25\n  :heartbeat-minutes 10\n  :context-atlas-path ".missiond/context/plan-rs.atlas.lisp"\n  :pattern-card-path ".missiond/patterns/schema-checker.pattern.lisp"\n  :context-pack-path ".missiond/tasks/wave28/context-pack.lisp"\n  :goal "Render a thin brief"\n  :write-scope ["scripts/x.mjs"]\n  :must-not-touch []\n  :requirements ["Use thin mode"]\n  :acceptance ["true"]\n  :commit (:required true :message "test: thin render" :scope-check write-scope-only)\n  :report ["Commit hash."])\n`;
      try {
        fs.writeFileSync(tmpTask, lisp, 'utf8');
        const task = loadSingleTask(tmpTask);
        const markdown = renderTask(task, tmpTask, {
          briefMode: 'thin',
          sharedPreamble: '.missiond/claudecode/wave28-shared-preamble.md',
        });
        const required = [
          'Thin brief rendered',
          '.missiond/claudecode/wave28-shared-preamble.md',
          'verification_tier',
          'heartbeat_minutes',
          'context_atlas',
          'pattern_card',
          'context_pack',
          '## Context Navigation',
          'integration-plan',
          'node scripts/task-scope-guard.mjs',
          'node scripts/verify-task-contract.mjs',
        ];
        for (const literal of required) {
          if (!markdown.includes(literal)) {
            throw new Error(`wave28 thin-brief invariant: missing '${literal}'`);
          }
        }
        const forbiddenSections = ['## Shared Memory', '## Report Contract', '## Session Trace', '## Router Policy'];
        for (const section of forbiddenSections) {
          if (markdown.includes(section)) {
            throw new Error(`wave28 thin-brief invariant: repeated boilerplate section '${section}' should be omitted`);
          }
        }
        const preamble = renderSharedPreamble();
        for (const literal of ['# MissionD ClaudeCode Shared Preamble', '## Shared Memory', '## Report Contract', '## Commit Protocol', 'shared preamble so preamble usage is auditable']) {
          if (!preamble.includes(literal)) {
            throw new Error(`wave28 preamble invariant: missing '${literal}'`);
          }
        }
      } finally {
        fs.rmSync(tmpDir, { recursive: true, force: true });
      }
    },
  });

  let failed = 0;
  const categories = new Set();
  for (const fixture of fixtures) {
    categories.add(fixture.category);
    try {
      fixture.run();
    } catch (err) {
      failed += 1;
      console.error(`fixture failed: ${fixture.name}`);
      console.error(`  ${err.message}`);
    }
  }
  if (failed > 0) {
    console.error(`render-claudecode-task fixtures FAILED — ${failed} of ${fixtures.length}`);
    process.exit(1);
  }
  console.log(
    `render-claudecode-task fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
  );
}

// wave28-03: gate main() on direct invocation so scripts/render-wave-briefs.mjs
// can import the renderer functions in-process (loadSingleTask / renderTask /
// renderSharedPreamble / deriveSharedPreamblePath) without triggering the CLI
// or shelling out. Direct `node scripts/render-claudecode-task.mjs ...` still
// runs main() exactly as before — every existing dry-fixture stays
// byte-identical at runtime.
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
