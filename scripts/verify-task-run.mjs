#!/usr/bin/env node

// MissionD task-run verifier (v1).
//
// Three-in-one post-run proof: ties together a task contract Lisp file, its
// machine report, the wave shared-memory ledger, and the actual git commit
// scope into a single structured verdict.
//
// Read-only by construction: this script never invokes any mutating git
// command (no add, commit, reset, checkout, stash, push, merge, rebase). The
// only git surface used is `git rev-parse`, `git log`, and `git show`,
// borrowed from scripts/verify-task-contract.mjs via its exported helpers.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

import {
  head,
  isList,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';
import {
  loadContract,
  loadContractFromSource,
  readCommit,
  verifyContract,
} from './verify-task-contract.mjs';
import {
  ENTRY_HEAD as TRACE_ENTRY_HEAD,
  KIND_VALUES as TRACE_KIND_VALUES,
  parseTraceEvents,
} from './check-session-trace.mjs';

const usage = `Usage:
  node scripts/verify-task-run.mjs \\
       --task <task.lisp> --report <report.lisp> \\
       --memory <shared-memory.lisp> --commit <hash> \\
       [--trace <session-trace.lisp>] [--require-trace] [--json]
  node scripts/verify-task-run.mjs --dry-fixture [--json]

Verifies a complete MissionD task run as a single post-run proof:
  1. task contract passes (delegates to verify-task-contract logic — message,
     :write-scope, :must-not-touch all checked against the actual commit)
  2. report :task_id matches the task contract id
  3. report :commit_hash matches the resolved commit (full or short prefix)
  4. shared-memory ledger contains a (completion ... :task <id>) entry that
     references the task
  5. (optional) session-trace contains at least one completion event for the
     task (kind=complete; failure events are NOT counted because this verifier
     confirms successful runs) and, when both report and trace carry a
     :commit_hash, they refer to the same commit (full sha or shared prefix
     of >=7 hex chars)

Flags:
  --task <task.lisp>            MissionD task-contract v1 file (required)
  --report <report.lisp>        MissionD report-contract v1 file (required)
  --memory <shared-memory.lisp> MissionD shared-memory v1 ledger (required
                                unless --allow-missing-memory is set)
  --commit <hash>               git ref to verify against; defaults to HEAD
  --trace <session-trace.lisp>  optional session-trace v1 ledger; when
                                supplied, the trace must contain at least
                                one (trace-event :task <id> :kind complete)
                                entry, and any commit_hash there must match
                                the report's commit_hash if both are present
  --require-trace               make absence of --trace (or absence of a
                                completion event for the task) a hard
                                failure; without this flag a missing trace
                                is silently allowed (no warning, no error)
  --allow-missing-memory        allow --memory to be omitted/missing; the
                                ledger check is skipped with a structured
                                warning instead of a hard failure
  --json                        emit machine-readable JSON instead of text
  --dry-fixture                 run self-contained fixtures (no git, no I/O)

This verifier is strictly read-only. It will never run any of:
  git add | git commit | git reset | git checkout | git stash |
  git push  | git merge  | git rebase | git rm | git mv | git tag
`;

const REPORT_SCHEMA = 'missiond.report-contract.v1';
const MEMORY_SCHEMA = 'missiond.shared-memory.v1';

function failUsage(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = {
    task: null,
    report: null,
    memory: null,
    commit: null,
    trace: null,
    requireTrace: false,
    json: false,
    dryFixture: false,
    allowMissingMemory: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '--dry-fixture') {
      opts.dryFixture = true;
    } else if (arg === '--allow-missing-memory') {
      opts.allowMissingMemory = true;
    } else if (arg === '--require-trace') {
      opts.requireTrace = true;
    } else if (arg === '--task') {
      opts.task = argv[++i] ?? failUsage('--task requires a value');
    } else if (arg.startsWith('--task=')) {
      opts.task = arg.slice('--task='.length);
    } else if (arg === '--report') {
      opts.report = argv[++i] ?? failUsage('--report requires a value');
    } else if (arg.startsWith('--report=')) {
      opts.report = arg.slice('--report='.length);
    } else if (arg === '--memory') {
      opts.memory = argv[++i] ?? failUsage('--memory requires a value');
    } else if (arg.startsWith('--memory=')) {
      opts.memory = arg.slice('--memory='.length);
    } else if (arg === '--commit') {
      opts.commit = argv[++i] ?? failUsage('--commit requires a value');
    } else if (arg.startsWith('--commit=')) {
      opts.commit = arg.slice('--commit='.length);
    } else if (arg === '--trace') {
      opts.trace = argv[++i] ?? failUsage('--trace requires a value');
    } else if (arg.startsWith('--trace=')) {
      opts.trace = arg.slice('--trace='.length);
    } else if (arg.startsWith('--')) {
      failUsage(`unknown flag: ${arg}`);
    } else {
      failUsage(`unexpected positional argument: ${arg}`);
    }
  }
  return opts;
}

// --- Report loading -------------------------------------------------------

export function loadReport(file) {
  const forms = readLispFile(file);
  return reportFromForms(forms, file);
}

export function loadReportFromSource(source, file = '<memory>') {
  const forms = parseLisp(source, file);
  return reportFromForms(forms, file);
}

function reportFromForms(forms, file) {
  const reportForm = forms.find((form) => isList(form) && head(form) === 'report');
  if (!reportForm) {
    throw new Error(`${file}: no (report ...) form found`);
  }
  const id = nodeText(reportForm.children[1]) ?? '<missing>';
  const props = readKeywordProps(reportForm, { start: 2 });
  const schema = keywordPropText(props, ':schema');
  if (schema !== REPORT_SCHEMA) {
    throw new Error(
      `${file}: report :schema must be "${REPORT_SCHEMA}", got ${JSON.stringify(schema)}`,
    );
  }
  const taskId = keywordPropText(props, ':task_id');
  const status = keywordPropText(props, ':status');
  const commitHash = keywordPropText(props, ':commit_hash');
  const agentCommitHash = keywordPropText(props, ':agent_commit_hash');
  const finalCommitHash = keywordPropText(props, ':final_commit_hash');
  const verifiedCommitHash = keywordPropText(props, ':verified_commit_hash');
  const filesChanged = nodeToStringArray(props[':files_changed']?.value);
  // wave29-04: extract :parent_patches structured entries so verifyRun can
  // (a) accept any lineage hash as "matched" against the resolved git
  // commit, and (b) surface the structured lineage in the JSON output.
  // Each entry projects to a flat shape so JSON consumers get the same
  // keys without having to walk the Lisp tree themselves.
  const parentPatches = parseParentPatches(props[':parent_patches']?.value);
  return {
    id,
    file,
    taskId,
    status,
    commitHash,
    agentCommitHash,
    finalCommitHash,
    verifiedCommitHash,
    parentPatches,
    filesChanged,
  };
}

function parseParentPatches(node) {
  if (node == null || !isList(node)) return [];
  const out = [];
  for (const entry of node.children) {
    if (!isList(entry)) continue;
    const ep = readKeywordProps(entry, { start: 0 });
    out.push({
      commit: keywordPropText(ep, ':commit') ?? null,
      kind: keywordPropText(ep, ':kind') ?? null,
      reason: keywordPropText(ep, ':reason') ?? null,
      files: ep[':files'] ? nodeToStringArray(ep[':files'].value) : [],
    });
  }
  return out;
}

// --- Shared-memory loading ------------------------------------------------

export function loadLedger(file) {
  const forms = readLispFile(file);
  return ledgerFromForms(forms, file);
}

export function loadLedgerFromSource(source, file = '<memory>') {
  const forms = parseLisp(source, file);
  return ledgerFromForms(forms, file);
}

function ledgerFromForms(forms, file) {
  const ledger = forms.find((form) => isList(form) && head(form) === 'shared-memory');
  if (!ledger) {
    throw new Error(`${file}: no (shared-memory ...) form found`);
  }
  const wave = nodeText(ledger.children[1]) ?? '<missing>';
  const props = readKeywordProps(ledger, { start: 2 });
  const schema = keywordPropText(props, ':schema');
  if (schema !== MEMORY_SCHEMA) {
    throw new Error(
      `${file}: shared-memory :schema must be "${MEMORY_SCHEMA}", got ${JSON.stringify(schema)}`,
    );
  }
  // Collect every (completion ...) entry with its :task and :id.
  const completions = [];
  for (const child of ledger.children) {
    if (!isList(child)) continue;
    if (head(child) !== 'completion') continue;
    const ep = readKeywordProps(child, { start: 1 });
    completions.push({
      id: keywordPropText(ep, ':id'),
      task: keywordPropText(ep, ':task'),
      seq: keywordPropText(ep, ':seq'),
      at: keywordPropText(ep, ':at'),
      summary: keywordPropText(ep, ':summary'),
    });
  }
  return { file, wave, completions };
}

// --- Session-trace loading ------------------------------------------------
//
// We import `parseTraceEvents` from scripts/check-session-trace.mjs (the
// shared parser exported in wave23-06) for file-based loading. For fixtures
// and tests we also need to parse a trace from an in-memory source string;
// `loadTraceFromSource` mirrors parseTraceEvents' return shape so both
// production and fixture paths feed the same `verifyRun` core.
//
// Invariant: this loader only extracts the trace-event entries and their
// scalar fields. Schema/seq/timestamp validation belong to
// scripts/check-session-trace.mjs; verify-task-run only consumes the parsed
// events and answers two questions:
//   1. Is there at least one (trace-event :task <id> :kind complete) entry?
//   2. If both report and that completion event carry :commit_hash, do they
//      reference the same commit?
//
// Kind decision: we accept ONLY :kind=complete as proof of a successful run.
// A `failure` event is itself a fact recorded in the trace, but it is the
// opposite of a completion proof — accepting it would make the verifier
// rubber-stamp aborted runs. The full kind enumeration (dispatch / start /
// read / edit / command / test / commit / complete / failure / retry /
// observation) is owned by check-session-trace.mjs (TRACE_KIND_VALUES) and
// we re-import it here so a typo in this file would surface as a stale
// reference rather than silently mismatching.
void TRACE_KIND_VALUES; // sanity import — kept live for future kind checks

export function loadTrace(file) {
  // parseTraceEvents returns an array of trace blocks (one per
  // (session-trace ...) form found). A single repo-relative trace file
  // typically has exactly one block, but we preserve the array shape.
  const traces = parseTraceEvents(file);
  return tracesToBundle(traces, file);
}

export function loadTraceFromSource(source, file = '<memory>') {
  const forms = parseLisp(source, file);
  const traces = [];
  for (const form of forms) {
    if (!isList(form) || head(form) !== 'session-trace') continue;
    const wave = nodeText(form.children[1]);
    const headerProps = readKeywordProps(form, { start: 2 });
    const header = {
      schema: keywordPropText(headerProps, ':schema'),
      wave: keywordPropText(headerProps, ':wave'),
      createdAt: keywordPropText(headerProps, ':created-at'),
      sequence: parseTraceInt(keywordPropText(headerProps, ':sequence')),
    };
    const events = [];
    for (const child of form.children) {
      if (!isList(child) || head(child) !== TRACE_ENTRY_HEAD) continue;
      const props = readKeywordProps(child, { start: 1 });
      events.push({
        id: keywordPropText(props, ':id'),
        seq: parseTraceInt(keywordPropText(props, ':seq')),
        at: keywordPropText(props, ':at'),
        task: keywordPropText(props, ':task'),
        backend: keywordPropText(props, ':backend'),
        kind: keywordPropText(props, ':kind'),
        summary: keywordPropText(props, ':summary'),
        agent: keywordPropText(props, ':agent'),
        files: props[':files'] ? nodeToStringArray(props[':files'].value) : [],
        command: keywordPropText(props, ':command'),
        exit_code: parseTraceInt(keywordPropText(props, ':exit_code')),
        duration_ms: parseTraceInt(keywordPropText(props, ':duration_ms')),
        commit_hash: keywordPropText(props, ':commit_hash'),
        report_path: keywordPropText(props, ':report_path'),
        memory_refs: props[':memory_refs']
          ? nodeToStringArray(props[':memory_refs'].value)
          : [],
        trace_refs: props[':trace_refs']
          ? nodeToStringArray(props[':trace_refs'].value)
          : [],
      });
    }
    traces.push({ file, wave, header, events });
  }
  return tracesToBundle(traces, file);
}

function tracesToBundle(traces, file) {
  const events = traces.flatMap((t) => t.events);
  return { file, traces, events };
}

function parseTraceInt(text) {
  if (text == null) return null;
  if (!/^-?\d+$/.test(text)) return null;
  return Number.parseInt(text, 10);
}

// --- Pure verification core ----------------------------------------------

export function verifyRun({
  contract,
  contractFile,
  report,
  reportFile,
  ledger,
  ledgerFile,
  ledgerStatus, // 'present' | 'missing-allowed' | 'missing-blocked'
  commitInfo,
  trace = null,
  traceFile = null,
  traceStatus = 'absent', // 'present' | 'absent' | 'absent-required' | 'malformed'
  traceLoadError = null,
  requireTrace = false,
}) {
  const errors = [];
  const warnings = [];
  const checks = {};

  // Check 1: task contract vs commit (delegates to verify-task-contract).
  const contractResult = verifyContract(contract, commitInfo);
  checks.task_contract = {
    ok: contractResult.ok,
    errors: contractResult.errors,
    warnings: contractResult.warnings,
  };
  if (!contractResult.ok) {
    for (const e of contractResult.errors) {
      errors.push(`task-contract: ${e}`);
    }
  }
  for (const w of contractResult.warnings) warnings.push(`task-contract: ${w}`);

  // Check 2: report :task_id == contract id.
  const taskIdMatch = report.taskId === contract.id;
  checks.report_task_id = {
    ok: taskIdMatch,
    expected: contract.id,
    got: report.taskId,
  };
  if (!taskIdMatch) {
    errors.push(
      `report :task_id mismatch — contract id is ${JSON.stringify(contract.id)}, ` +
      `report ${reportFile} has :task_id ${JSON.stringify(report.taskId)}`,
    );
  }

  // Check 3: report commit lineage matches resolved commit.
  //
  // wave29-04: accept ANY hash carried in the report's commit lineage as a
  // "matched" commit against the resolved git commit. The lineage is the
  // ordered tuple (commit_hash, agent_commit_hash, parent_patches[*].commit).
  // Acceptance is read-only — the verifier still emits the resolved
  // (final/verified) git hash as the authoritative commit, and a separate
  // lineage_match field tells callers WHICH lineage role matched. This lets
  // operators verify a worker commit OR a parent hotfix commit OR an
  // intermediate parent patch commit, while always reporting the canonical
  // resolved hash. Reports without lineage fields fall back to the original
  // wave21 single-hash check (commit_hash only).
  const lineageEntries = buildLineageEntries(report);
  const matchedLineage = lineageEntries.find((entry) =>
    commitHashMatches(entry.hash, commitInfo.hash),
  );
  const commitMatch = Boolean(matchedLineage);
  checks.report_commit_hash = {
    ok: commitMatch,
    expected_full: commitInfo.hash,
    got: report.commitHash,
    lineage_match: matchedLineage
      ? { role: matchedLineage.role, hash: matchedLineage.hash, index: matchedLineage.index ?? null }
      : null,
  };
  if (!commitMatch) {
    const lineageJson = JSON.stringify(lineageEntries.map((e) => ({ role: e.role, hash: e.hash })));
    errors.push(
      `report :commit_hash mismatch — resolved commit is ${commitInfo.hash}, ` +
      `report lineage ${lineageJson} carries no matching hash`,
    );
  }
  // Always surface the structured lineage so JSON consumers can audit which
  // hashes the report carried, regardless of which one matched.
  checks.report_commit_lineage = {
    agent_commit_hash: report.agentCommitHash ?? null,
    final_commit_hash: report.finalCommitHash ?? null,
    verified_commit_hash: report.verifiedCommitHash ?? null,
    parent_patches: (report.parentPatches ?? []).map((p) => ({
      commit: p.commit,
      kind: p.kind,
      reason: p.reason,
      files: p.files,
    })),
  };

  // Check 4: report :status (informational; done is the contracted happy path).
  if (report.status && report.status !== 'done') {
    warnings.push(
      `report :status is ${JSON.stringify(report.status)}; expected "done" for a full run proof`,
    );
  }

  // Check 5: shared-memory completion entry for the task.
  if (ledgerStatus === 'present') {
    const matched = ledger.completions.find((c) => c.task === contract.id);
    checks.shared_memory_completion = {
      ok: Boolean(matched),
      task: contract.id,
      ledger_file: ledgerFile,
      matched_entry_id: matched?.id ?? null,
    };
    if (!matched) {
      errors.push(
        `shared-memory ledger ${ledgerFile} has no (completion :task ${contract.id} ...) entry`,
      );
    }
  } else if (ledgerStatus === 'missing-allowed') {
    checks.shared_memory_completion = {
      ok: true,
      skipped: true,
      reason: '--allow-missing-memory was passed',
    };
    warnings.push(
      'shared-memory ledger check skipped (--allow-missing-memory); ' +
      'task run is NOT fully proven without a completion entry',
    );
  } else {
    // missing-blocked
    checks.shared_memory_completion = {
      ok: false,
      missing: true,
      reason: 'shared-memory ledger missing or unreadable',
    };
    errors.push(
      'shared-memory ledger missing or unreadable; pass --allow-missing-memory ' +
      'to skip this check explicitly',
    );
  }

  // Check 6: session-trace completion event for the task (and commit_hash
  // cross-check). The trace check is opt-in: by default a missing trace is
  // silently allowed (traceStatus='absent'). When --require-trace is set,
  // an absent or malformed trace becomes a hard failure
  // (traceStatus='absent-required' or 'malformed').
  if (traceStatus === 'present') {
    const taskEvents = trace.events.filter((e) => e.task === contract.id);
    // Only :kind=complete proves a successful run. :kind=failure is the
    // opposite of a completion proof and is intentionally rejected here —
    // see the kind-decision comment near loadTrace.
    const completion = taskEvents.find((e) => e.kind === 'complete');
    checks.session_trace_completion = {
      ok: Boolean(completion),
      task: contract.id,
      trace_file: traceFile,
      task_event_count: taskEvents.length,
      matched_event_id: completion?.id ?? null,
      matched_event_seq: completion?.seq ?? null,
    };
    if (!completion) {
      const failureCount = taskEvents.filter((e) => e.kind === 'failure').length;
      const sample = failureCount > 0 ? ` (${failureCount} failure event(s) present)` : '';
      const message =
        `session-trace ${traceFile} has no (trace-event :task ${contract.id} :kind complete) entry${sample}`;
      // If --trace was passed explicitly we always treat absence of a
      // completion event for this task as an error: the operator opted in
      // to the trace check, so a missing completion is a real defect, not
      // silent telemetry. --require-trace is the orthogonal "trace must be
      // supplied" gate; the present-but-no-completion case is already a
      // hard signal from the operator.
      errors.push(message);
      // Mark the check explicitly per requireTrace so the JSON output
      // distinguishes the two failure modes for tooling.
      checks.session_trace_completion.required = requireTrace;
    } else {
      // Check 7: commit_hash cross-check. Only fail when BOTH sides have a
      // hash and they disagree. If only one side carries a hash, that's a
      // soft warning (allowed) — the report-vs-git commit_hash check above
      // already enforces the report side, and the trace's commit_hash is
      // optional per the schema.
      const traceHash = completion.commit_hash;
      const reportHash = report.commitHash;
      if (traceHash && reportHash) {
        const matches = commitHashesAgree(traceHash, reportHash);
        checks.session_trace_commit_hash = {
          ok: matches,
          report_hash: reportHash,
          trace_hash: traceHash,
        };
        if (!matches) {
          errors.push(
            `session-trace commit_hash mismatch — trace event ${completion.id} has ` +
            `${JSON.stringify(traceHash)}, report has ${JSON.stringify(reportHash)}`,
          );
        }
      } else if (traceHash || reportHash) {
        checks.session_trace_commit_hash = {
          ok: true,
          skipped: true,
          report_hash: reportHash ?? null,
          trace_hash: traceHash ?? null,
          reason: 'only one side carries :commit_hash; cross-check skipped',
        };
        warnings.push(
          'session-trace and report commit_hash cross-check skipped — only ' +
          (traceHash ? 'trace' : 'report') + ' carries :commit_hash',
        );
      }
      // else: neither side has a hash. The report-vs-git check (above) is
      // the source of truth; nothing to assert here.
    }
  } else if (traceStatus === 'absent') {
    checks.session_trace_completion = {
      ok: true,
      skipped: true,
      reason: '--trace not provided and --require-trace not set',
    };
  } else if (traceStatus === 'absent-required') {
    checks.session_trace_completion = {
      ok: false,
      missing: true,
      reason: '--require-trace set but --trace not provided',
    };
    errors.push(
      'session-trace required but not provided; pass --trace <session-trace.lisp> ' +
      'or drop --require-trace',
    );
  } else if (traceStatus === 'malformed') {
    checks.session_trace_completion = {
      ok: false,
      malformed: true,
      reason: traceLoadError ?? 'session-trace failed to parse',
    };
    errors.push(`session-trace failed to load: ${traceLoadError ?? 'unknown error'}`);
  }

  return {
    ok: errors.length === 0,
    contract_file: contractFile,
    task_id: contract.id,
    commit: commitInfo.hash,
    checks,
    errors,
    warnings,
  };
}

function commitHashesAgree(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return false;
  const ax = a.trim().toLowerCase();
  const bx = b.trim().toLowerCase();
  if (ax.length === 0 || bx.length === 0) return false;
  if (!/^[0-9a-f]+$/.test(ax) || !/^[0-9a-f]+$/.test(bx)) return false;
  if (ax === bx) return true;
  // Allow either side to be a >=7-hex prefix of the other (e.g. report has
  // short SHA and trace has full SHA, or vice versa).
  const longer = ax.length >= bx.length ? ax : bx;
  const shorter = ax.length >= bx.length ? bx : ax;
  if (shorter.length >= 7 && longer.startsWith(shorter)) return true;
  return false;
}

function commitHashMatches(reported, full) {
  if (!reported || typeof reported !== 'string') return false;
  const r = reported.trim().toLowerCase();
  const f = full.trim().toLowerCase();
  if (r.length === 0) return false;
  // Accept full sha equality or a prefix of the full sha (>= 7 hex chars).
  if (r === f) return true;
  if (r.length >= 7 && /^[0-9a-f]+$/.test(r) && f.startsWith(r)) return true;
  return false;
}

// wave29-04 helper: enumerate the report's commit lineage in a stable
// canonical order so verifyRun can accept any of them as a "matched" commit
// while still reporting which role matched. Order: commit_hash (final),
// agent_commit_hash (worker), parent_patches[*].commit (intermediate). The
// caller filters out null entries; entries are de-duplicated by hash so the
// report-vs-git check does not pay for re-comparing the same hash twice.
function buildLineageEntries(report) {
  const seen = new Set();
  const out = [];
  const push = (role, hash, index = null) => {
    if (typeof hash !== 'string' || hash.trim() === '') return;
    const key = hash.trim().toLowerCase();
    if (seen.has(key)) return;
    seen.add(key);
    out.push({ role, hash, index });
  };
  push('commit_hash', report.commitHash);
  push('agent_commit_hash', report.agentCommitHash);
  const patches = report.parentPatches ?? [];
  for (let i = 0; i < patches.length; i++) {
    push('parent_patch', patches[i].commit, i);
  }
  return out;
}

// --- Output ---------------------------------------------------------------

function emit(payload, { json }) {
  if (json) {
    console.log(JSON.stringify(payload, null, 2));
    return;
  }
  if (payload.ok) {
    console.log(
      `task-run verify OK: ${payload.task_id} against ${String(payload.commit).slice(0, 12)}`,
    );
    if (payload.warnings && payload.warnings.length) {
      for (const w of payload.warnings) console.warn(`warn: ${w}`);
    }
    return;
  }
  console.error(`task-run verify FAILED: ${payload.task_id ?? payload.contract_file}`);
  for (const e of payload.errors ?? []) console.error(`  ${e}`);
  if (payload.warnings && payload.warnings.length) {
    for (const w of payload.warnings) console.warn(`warn: ${w}`);
  }
}

// --- CLI ------------------------------------------------------------------

function runCli(opts) {
  if (!opts.task) failUsage('--task <task.lisp> is required (or use --dry-fixture)');
  if (!opts.report) failUsage('--report <report.lisp> is required (or use --dry-fixture)');

  const cwd = process.cwd();
  const taskFile = path.resolve(cwd, opts.task);
  const reportFile = path.resolve(cwd, opts.report);

  let contract;
  try {
    contract = loadContract(taskFile);
  } catch (err) {
    emit(
      {
        ok: false,
        contract_file: taskFile,
        task_id: null,
        commit: null,
        errors: [`failed to load task contract: ${err.message ?? err}`],
        warnings: [],
        checks: {},
      },
      { json: opts.json },
    );
    process.exit(1);
  }

  let report;
  try {
    report = loadReport(reportFile);
  } catch (err) {
    emit(
      {
        ok: false,
        contract_file: taskFile,
        task_id: contract.id,
        commit: null,
        errors: [`failed to load report: ${err.message ?? err}`],
        warnings: [],
        checks: {},
      },
      { json: opts.json },
    );
    process.exit(1);
  }

  // Memory: explicit-missing path is part of the contract.
  let ledger = null;
  let ledgerFile = opts.memory ? path.resolve(cwd, opts.memory) : null;
  let ledgerStatus = 'present';
  if (!ledgerFile) {
    if (opts.allowMissingMemory) {
      ledgerStatus = 'missing-allowed';
    } else {
      emit(
        {
          ok: false,
          contract_file: taskFile,
          task_id: contract.id,
          commit: null,
          errors: [
            '--memory <shared-memory.lisp> is required; pass --allow-missing-memory to skip explicitly',
          ],
          warnings: [],
          checks: {
            shared_memory_completion: {
              ok: false,
              missing: true,
              reason: '--memory not provided',
            },
          },
        },
        { json: opts.json },
      );
      process.exit(1);
    }
  } else if (!fs.existsSync(ledgerFile)) {
    if (opts.allowMissingMemory) {
      ledgerStatus = 'missing-allowed';
    } else {
      emit(
        {
          ok: false,
          contract_file: taskFile,
          task_id: contract.id,
          commit: null,
          errors: [
            `shared-memory ledger not found at ${ledgerFile}; ` +
            `pass --allow-missing-memory to skip explicitly`,
          ],
          warnings: [],
          checks: {
            shared_memory_completion: {
              ok: false,
              missing: true,
              reason: `not found at ${ledgerFile}`,
            },
          },
        },
        { json: opts.json },
      );
      process.exit(1);
    }
  } else {
    try {
      ledger = loadLedger(ledgerFile);
    } catch (err) {
      emit(
        {
          ok: false,
          contract_file: taskFile,
          task_id: contract.id,
          commit: null,
          errors: [`failed to load shared-memory ledger: ${err.message ?? err}`],
          warnings: [],
          checks: {},
        },
        { json: opts.json },
      );
      process.exit(1);
    }
  }

  let commitInfo;
  try {
    commitInfo = readCommit(opts.commit ?? 'HEAD');
  } catch (err) {
    emit(
      {
        ok: false,
        contract_file: taskFile,
        task_id: contract.id,
        commit: opts.commit ?? 'HEAD',
        errors: [`failed to read git commit ${opts.commit ?? 'HEAD'}: ${err.message ?? err}`],
        warnings: [],
        checks: {},
      },
      { json: opts.json },
    );
    process.exit(1);
  }

  // Trace: optional input. --require-trace promotes any absence (or load
  // failure) to an error; without it an absent --trace is silently OK.
  let trace = null;
  let traceFile = opts.trace ? path.resolve(cwd, opts.trace) : null;
  let traceStatus = 'absent';
  let traceLoadError = null;
  if (traceFile) {
    if (!fs.existsSync(traceFile)) {
      traceStatus = 'malformed';
      traceLoadError = `session-trace not found at ${traceFile}`;
    } else {
      try {
        trace = loadTrace(traceFile);
        traceStatus = 'present';
      } catch (err) {
        traceStatus = 'malformed';
        traceLoadError = err?.message ?? String(err);
      }
    }
  } else if (opts.requireTrace) {
    traceStatus = 'absent-required';
  }

  const result = verifyRun({
    contract,
    contractFile: taskFile,
    report,
    reportFile,
    ledger,
    ledgerFile,
    ledgerStatus,
    commitInfo,
    trace,
    traceFile,
    traceStatus,
    traceLoadError,
    requireTrace: opts.requireTrace,
  });

  emit(result, { json: opts.json });
  process.exit(result.ok ? 0 : 1);
}

// --- Fixtures -------------------------------------------------------------

// Builds a minimal session-trace source for fixture use. `kind` selects the
// :kind of the single (trace-event ...). When `commitHash` is supplied it is
// emitted as :commit_hash. Other optional fields are intentionally omitted
// to keep fixture diffs scoped to the behavior under test.
function buildTraceSource({
  task,
  kind = 'complete',
  commitHash = null,
  wave = 'wave21',
  eventId = 'wave21-trace-001',
  seq = 1,
  at = '2026-04-28T00:00:00Z',
  backend = 'claudecode',
  summary = 'fixture',
} = {}) {
  const commitLine = commitHash ? `\n      :commit_hash "${commitHash}"` : '';
  return `(session-trace ${wave}
    :schema "missiond.session-trace.v1"
    :wave ${wave}
    :created-at "2026-04-28T00:00:00Z"
    :sequence 1
    (trace-event
      :id ${eventId}
      :seq ${seq}
      :at "${at}"
      :task ${task}
      :backend ${backend}
      :kind ${kind}
      :summary "${summary}"${commitLine}))`;
}

function runFixtures({ json }) {
  const baseTaskSource = `(task wave21-99-fixture
    :schema "missiond.task-contract.v1"
    :title "Run-verifier fixture"
    :kind code-alignment
    :status ready
    :owner "claudecode"
    :goal "fixture"
    :write-scope ["scripts/verify-task-run.mjs" "scripts/lib/**"]
    :must-not-touch ["crates/**" ".missiond/v2/*.lisp"]
    :acceptance ["true"]
    :commit (:required true
             :message "feat(tasks): verify complete task runs"
             :scope-check write-scope-only))`;

  const baseReportSource = `(report wave21-99-fixture
    :schema "missiond.report-contract.v1"
    :task_id "wave21-99-fixture"
    :status done
    :commit_hash "abc1234"
    :files_changed ["scripts/verify-task-run.mjs"]
    :acceptance_results
      [(:command "node scripts/verify-task-run.mjs --dry-fixture"
        :exit_code 0 :ok true)])`;

  const baseLedgerSource = `(shared-memory wave21
    :schema "missiond.shared-memory.v1"
    :wave wave21
    :created-at "2026-04-26T00:00:00Z"
    :sequence 1
    (claim
      :id wave21-99-claim-001
      :task wave21-99-fixture
      :agent claudecode
      :seq 1
      :summary "claim")
    (completion
      :id wave21-99-completion-001
      :task wave21-99-fixture
      :agent claudecode
      :seq 2
      :touched ["scripts/verify-task-run.mjs"]
      :summary "done"))`;

  const baseCommit = {
    hash: 'abc1234567890abcdef1234567890abcdef12345',
    message: 'feat(tasks): verify complete task runs\n\nbody\n',
    files: ['scripts/verify-task-run.mjs'],
  };

  const fixtures = [
    {
      name: 'all-green: contract + report + ledger + commit aligned',
      contract: loadContractFromSource(baseTaskSource, '<fx-base-task>'),
      contractFile: '<fx-base-task>',
      report: loadReportFromSource(baseReportSource, '<fx-base-report>'),
      reportFile: '<fx-base-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-base-ledger>'),
      ledgerFile: '<fx-base-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      expectOk: true,
    },
    {
      name: 'report task_id mismatch',
      contract: loadContractFromSource(baseTaskSource, '<fx-mm-task>'),
      contractFile: '<fx-mm-task>',
      report: loadReportFromSource(
        baseReportSource.replace(':task_id "wave21-99-fixture"', ':task_id "wave21-99-other"'),
        '<fx-mm-report>',
      ),
      reportFile: '<fx-mm-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-mm-ledger>'),
      ledgerFile: '<fx-mm-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      expectOk: false,
      expectError: /report :task_id mismatch/,
    },
    {
      name: 'report commit_hash mismatch',
      contract: loadContractFromSource(baseTaskSource, '<fx-ch-task>'),
      contractFile: '<fx-ch-task>',
      report: loadReportFromSource(
        baseReportSource.replace(':commit_hash "abc1234"', ':commit_hash "deadbee"'),
        '<fx-ch-report>',
      ),
      reportFile: '<fx-ch-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-ch-ledger>'),
      ledgerFile: '<fx-ch-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      expectOk: false,
      expectError: /report :commit_hash mismatch/,
    },
    {
      name: 'report commit_hash full sha matches',
      contract: loadContractFromSource(baseTaskSource, '<fx-full-task>'),
      contractFile: '<fx-full-task>',
      report: loadReportFromSource(
        baseReportSource.replace(
          ':commit_hash "abc1234"',
          `:commit_hash "${baseCommit.hash}"`,
        ),
        '<fx-full-report>',
      ),
      reportFile: '<fx-full-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-full-ledger>'),
      ledgerFile: '<fx-full-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      expectOk: true,
    },
    {
      name: 'commit message mismatch (delegated to task-contract verifier)',
      contract: loadContractFromSource(baseTaskSource, '<fx-msg-task>'),
      contractFile: '<fx-msg-task>',
      report: loadReportFromSource(baseReportSource, '<fx-msg-report>'),
      reportFile: '<fx-msg-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-msg-ledger>'),
      ledgerFile: '<fx-msg-ledger>',
      ledgerStatus: 'present',
      commitInfo: { ...baseCommit, message: 'chore: nope\n' },
      expectOk: false,
      expectError: /task-contract: commit message does not match/,
    },
    {
      name: 'file outside write-scope (delegated)',
      contract: loadContractFromSource(baseTaskSource, '<fx-scope-task>'),
      contractFile: '<fx-scope-task>',
      report: loadReportFromSource(baseReportSource, '<fx-scope-report>'),
      reportFile: '<fx-scope-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-scope-ledger>'),
      ledgerFile: '<fx-scope-ledger>',
      ledgerStatus: 'present',
      commitInfo: {
        ...baseCommit,
        files: ['scripts/verify-task-run.mjs', 'README.md'],
      },
      expectOk: false,
      expectError: /task-contract: commit touches files outside :write-scope/,
    },
    {
      name: 'commit hits must-not-touch (delegated)',
      contract: loadContractFromSource(baseTaskSource, '<fx-mnt-task>'),
      contractFile: '<fx-mnt-task>',
      report: loadReportFromSource(baseReportSource, '<fx-mnt-report>'),
      reportFile: '<fx-mnt-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-mnt-ledger>'),
      ledgerFile: '<fx-mnt-ledger>',
      ledgerStatus: 'present',
      commitInfo: {
        ...baseCommit,
        files: ['scripts/verify-task-run.mjs', 'crates/missiond-core/src/lib.rs'],
      },
      expectOk: false,
      expectError: /task-contract: commit touches files inside :must-not-touch/,
    },
    {
      name: 'shared-memory missing completion entry',
      contract: loadContractFromSource(baseTaskSource, '<fx-noc-task>'),
      contractFile: '<fx-noc-task>',
      report: loadReportFromSource(baseReportSource, '<fx-noc-report>'),
      reportFile: '<fx-noc-report>',
      ledger: loadLedgerFromSource(
        `(shared-memory wave21
          :schema "missiond.shared-memory.v1"
          :wave wave21
          :created-at "2026-04-26T00:00:00Z"
          :sequence 1
          (claim
            :id wave21-99-claim-001
            :task wave21-99-fixture
            :agent claudecode
            :seq 1
            :summary "claim only"))`,
        '<fx-noc-ledger>',
      ),
      ledgerFile: '<fx-noc-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      expectOk: false,
      expectError: /no \(completion :task wave21-99-fixture/,
    },
    {
      name: 'shared-memory ledger has completion for a different task',
      contract: loadContractFromSource(baseTaskSource, '<fx-other-task>'),
      contractFile: '<fx-other-task>',
      report: loadReportFromSource(baseReportSource, '<fx-other-report>'),
      reportFile: '<fx-other-report>',
      ledger: loadLedgerFromSource(
        `(shared-memory wave21
          :schema "missiond.shared-memory.v1"
          :wave wave21
          :created-at "2026-04-26T00:00:00Z"
          :sequence 1
          (completion
            :id wave21-98-completion-001
            :task wave21-98-other
            :agent claudecode
            :seq 1
            :touched ["x"]
            :summary "done"))`,
        '<fx-other-ledger>',
      ),
      ledgerFile: '<fx-other-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      expectOk: false,
      expectError: /no \(completion :task wave21-99-fixture/,
    },
    {
      name: '--allow-missing-memory skips ledger check with warning',
      contract: loadContractFromSource(baseTaskSource, '<fx-skip-task>'),
      contractFile: '<fx-skip-task>',
      report: loadReportFromSource(baseReportSource, '<fx-skip-report>'),
      reportFile: '<fx-skip-report>',
      ledger: null,
      ledgerFile: null,
      ledgerStatus: 'missing-allowed',
      commitInfo: baseCommit,
      expectOk: true,
      expectWarning: /shared-memory ledger check skipped/,
    },
    {
      name: 'missing memory without allow flag fails with structured diagnostic',
      contract: loadContractFromSource(baseTaskSource, '<fx-block-task>'),
      contractFile: '<fx-block-task>',
      report: loadReportFromSource(baseReportSource, '<fx-block-report>'),
      reportFile: '<fx-block-report>',
      ledger: null,
      ledgerFile: null,
      ledgerStatus: 'missing-blocked',
      commitInfo: baseCommit,
      expectOk: false,
      expectError: /shared-memory ledger missing or unreadable/,
    },
    {
      name: 'report status not done emits warning but is not fatal',
      contract: loadContractFromSource(baseTaskSource, '<fx-status-task>'),
      contractFile: '<fx-status-task>',
      report: loadReportFromSource(
        baseReportSource.replace(':status done', ':status in-progress'),
        '<fx-status-report>',
      ),
      reportFile: '<fx-status-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-status-ledger>'),
      ledgerFile: '<fx-status-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      expectOk: true,
      expectWarning: /report :status is "in-progress"/,
    },

    // ---------- session-trace fixtures (wave23-03) ----------
    {
      name: 'trace pass: contract+report+memory+commit+trace with matching completion + matching commit_hash',
      contract: loadContractFromSource(baseTaskSource, '<fx-tr-pass-task>'),
      contractFile: '<fx-tr-pass-task>',
      report: loadReportFromSource(baseReportSource, '<fx-tr-pass-report>'),
      reportFile: '<fx-tr-pass-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-tr-pass-ledger>'),
      ledgerFile: '<fx-tr-pass-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      trace: loadTraceFromSource(
        buildTraceSource({ task: 'wave21-99-fixture', kind: 'complete', commitHash: 'abc1234' }),
        '<fx-tr-pass-trace>',
      ),
      traceFile: '<fx-tr-pass-trace>',
      traceStatus: 'present',
      requireTrace: true,
      expectOk: true,
    },
    {
      name: 'trace fail: trace supplied but no completion event for this task',
      contract: loadContractFromSource(baseTaskSource, '<fx-tr-noc-task>'),
      contractFile: '<fx-tr-noc-task>',
      report: loadReportFromSource(baseReportSource, '<fx-tr-noc-report>'),
      reportFile: '<fx-tr-noc-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-tr-noc-ledger>'),
      ledgerFile: '<fx-tr-noc-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      // Trace has only a `failure` event for this task; verifier rejects.
      trace: loadTraceFromSource(
        buildTraceSource({ task: 'wave21-99-fixture', kind: 'failure' }),
        '<fx-tr-noc-trace>',
      ),
      traceFile: '<fx-tr-noc-trace>',
      traceStatus: 'present',
      requireTrace: false,
      expectOk: false,
      expectError: /no \(trace-event :task wave21-99-fixture :kind complete\) entry/,
    },
    {
      name: 'trace fail: report and trace both have commit_hash but they mismatch',
      contract: loadContractFromSource(baseTaskSource, '<fx-tr-cm-task>'),
      contractFile: '<fx-tr-cm-task>',
      report: loadReportFromSource(baseReportSource, '<fx-tr-cm-report>'),
      reportFile: '<fx-tr-cm-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-tr-cm-ledger>'),
      ledgerFile: '<fx-tr-cm-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      trace: loadTraceFromSource(
        // trace says deadbee, report says abc1234 → cross-check fires.
        buildTraceSource({ task: 'wave21-99-fixture', kind: 'complete', commitHash: 'deadbee' }),
        '<fx-tr-cm-trace>',
      ),
      traceFile: '<fx-tr-cm-trace>',
      traceStatus: 'present',
      requireTrace: false,
      expectOk: false,
      expectError: /session-trace commit_hash mismatch/,
    },
    {
      name: 'trace fail: malformed trace (parser error)',
      contract: loadContractFromSource(baseTaskSource, '<fx-tr-mal-task>'),
      contractFile: '<fx-tr-mal-task>',
      report: loadReportFromSource(baseReportSource, '<fx-tr-mal-report>'),
      reportFile: '<fx-tr-mal-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-tr-mal-ledger>'),
      ledgerFile: '<fx-tr-mal-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      trace: null,
      traceFile: '<fx-tr-mal-trace>',
      traceStatus: 'malformed',
      traceLoadError: 'unmatched paren at line 12',
      requireTrace: false,
      expectOk: false,
      expectError: /session-trace failed to load/,
    },
    {
      name: 'trace pass: trace absent and --require-trace not set',
      contract: loadContractFromSource(baseTaskSource, '<fx-tr-abs-task>'),
      contractFile: '<fx-tr-abs-task>',
      report: loadReportFromSource(baseReportSource, '<fx-tr-abs-report>'),
      reportFile: '<fx-tr-abs-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-tr-abs-ledger>'),
      ledgerFile: '<fx-tr-abs-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      trace: null,
      traceFile: null,
      traceStatus: 'absent',
      requireTrace: false,
      expectOk: true,
    },
    {
      name: 'trace fail: trace absent and --require-trace set',
      contract: loadContractFromSource(baseTaskSource, '<fx-tr-req-task>'),
      contractFile: '<fx-tr-req-task>',
      report: loadReportFromSource(baseReportSource, '<fx-tr-req-report>'),
      reportFile: '<fx-tr-req-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-tr-req-ledger>'),
      ledgerFile: '<fx-tr-req-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      trace: null,
      traceFile: null,
      traceStatus: 'absent-required',
      requireTrace: true,
      expectOk: false,
      expectError: /session-trace required but not provided/,
    },
    {
      name: 'trace pass with warning: only trace has commit_hash (report side empty)',
      contract: loadContractFromSource(baseTaskSource, '<fx-tr-only-task>'),
      contractFile: '<fx-tr-only-task>',
      // Report has commit_hash matching the actual commit, but we strip
      // the trace's commit_hash claim to exercise the asymmetric branch in
      // reverse: only report-side has a hash.
      report: loadReportFromSource(baseReportSource, '<fx-tr-only-report>'),
      reportFile: '<fx-tr-only-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-tr-only-ledger>'),
      ledgerFile: '<fx-tr-only-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      trace: loadTraceFromSource(
        // No :commit_hash on the completion event — only one side carries
        // a hash, cross-check is skipped with a warning.
        buildTraceSource({ task: 'wave21-99-fixture', kind: 'complete' }),
        '<fx-tr-only-trace>',
      ),
      traceFile: '<fx-tr-only-trace>',
      traceStatus: 'present',
      requireTrace: false,
      expectOk: true,
      expectWarning: /commit_hash cross-check skipped/,
    },

    // ---------- wave29-04 lineage fixtures ----------
    // Pin the wave28-02 hotfix exemplar shape: worker commit 954116e then
    // parent lint-cleanup commit 302330a. The resolved git commit in this
    // fixture is the FINAL commit; the report's :commit_hash also matches
    // the final commit, while :agent_commit_hash carries the worker hash
    // and :parent_patches[0].commit carries the final commit (matching the
    // wave28-02 on-disk report shape that wave29-04 hardens).
    {
      name: 'wave29-04 lineage pass: report :commit_hash + agent + parent_patches all aligned to final',
      contract: loadContractFromSource(baseTaskSource, '<fx-lin-pass-task>'),
      contractFile: '<fx-lin-pass-task>',
      report: loadReportFromSource(
        `(report wave21-99-fixture
          :schema "missiond.report-contract.v1"
          :task_id "wave21-99-fixture"
          :status done
          :commit_hash "abc1234"
          :agent_commit_hash "deadbee0001"
          :parent_patches
            [(:commit "abc1234"
              :kind lint-cleanup
              :reason "TS6133 cleanup"
              :files ["scripts/verify-task-run.mjs"])]
          :files_changed ["scripts/verify-task-run.mjs"]
          :acceptance_results
            [(:command "node scripts/verify-task-run.mjs --dry-fixture" :exit_code 0 :ok true)])`,
        '<fx-lin-pass-report>',
      ),
      reportFile: '<fx-lin-pass-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-lin-pass-ledger>'),
      ledgerFile: '<fx-lin-pass-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      expectOk: true,
    },
    // Worker-only verification: resolved git commit is the WORKER commit
    // (deadbee...), report :commit_hash points at the FINAL hash but
    // :agent_commit_hash carries the worker hash. The lineage-aware match
    // accepts the worker commit as a valid lineage hit; verifier still
    // emits the resolved hash as authoritative.
    {
      name: 'wave29-04 lineage pass: agent_commit_hash matches resolved (worker) commit',
      contract: loadContractFromSource(baseTaskSource, '<fx-lin-agent-task>'),
      contractFile: '<fx-lin-agent-task>',
      report: loadReportFromSource(
        `(report wave21-99-fixture
          :schema "missiond.report-contract.v1"
          :task_id "wave21-99-fixture"
          :status done
          :commit_hash "feedbabe"
          :agent_commit_hash "deadbee0001"
          :parent_patches
            [(:commit "feedbabe"
              :kind lint-cleanup
              :reason "post-worker hotfix"
              :files ["scripts/verify-task-run.mjs"])]
          :files_changed ["scripts/verify-task-run.mjs"]
          :acceptance_results
            [(:command "node scripts/verify-task-run.mjs --dry-fixture" :exit_code 0 :ok true)])`,
        '<fx-lin-agent-report>',
      ),
      reportFile: '<fx-lin-agent-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-lin-agent-ledger>'),
      ledgerFile: '<fx-lin-agent-ledger>',
      ledgerStatus: 'present',
      // Resolved commit is the WORKER commit (deadbee...), not the final.
      commitInfo: {
        hash: 'deadbee00011234567890abcdef1234567890abcd',
        message: baseCommit.message,
        files: baseCommit.files,
      },
      expectOk: true,
    },
    // Negative: resolved commit is NEITHER the report's commit_hash NOR any
    // lineage hash. The error message MUST mention "report lineage" so the
    // failure is debuggable.
    {
      name: 'wave29-04 lineage fail: resolved commit not in any lineage hash',
      contract: loadContractFromSource(baseTaskSource, '<fx-lin-miss-task>'),
      contractFile: '<fx-lin-miss-task>',
      report: loadReportFromSource(
        `(report wave21-99-fixture
          :schema "missiond.report-contract.v1"
          :task_id "wave21-99-fixture"
          :status done
          :commit_hash "abc1234"
          :agent_commit_hash "deadbee"
          :parent_patches
            [(:commit "abc1234"
              :kind lint-cleanup
              :reason "x"
              :files ["scripts/verify-task-run.mjs"])]
          :files_changed ["scripts/verify-task-run.mjs"]
          :acceptance_results
            [(:command "true" :exit_code 0 :ok true)])`,
        '<fx-lin-miss-report>',
      ),
      reportFile: '<fx-lin-miss-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-lin-miss-ledger>'),
      ledgerFile: '<fx-lin-miss-ledger>',
      ledgerStatus: 'present',
      commitInfo: {
        hash: 'cafebabe000011223344556677889900aabbccdd',
        message: baseCommit.message,
        files: baseCommit.files,
      },
      expectOk: false,
      expectError: /report lineage .* carries no matching hash/,
    },
    // Backward-compat: a report WITHOUT any lineage fields still verifies
    // green when :commit_hash matches resolved git commit. The lineage
    // entries collapse to a single role=commit_hash entry.
    {
      name: 'wave29-04 lineage pass: legacy report (no lineage fields) matches via commit_hash',
      contract: loadContractFromSource(baseTaskSource, '<fx-lin-legacy-task>'),
      contractFile: '<fx-lin-legacy-task>',
      report: loadReportFromSource(baseReportSource, '<fx-lin-legacy-report>'),
      reportFile: '<fx-lin-legacy-report>',
      ledger: loadLedgerFromSource(baseLedgerSource, '<fx-lin-legacy-ledger>'),
      ledgerFile: '<fx-lin-legacy-ledger>',
      ledgerStatus: 'present',
      commitInfo: baseCommit,
      expectOk: true,
    },
  ];

  // Helper-level sanity check on commitHashMatches.
  const helperCases = [
    ['abc1234', 'abc1234567890abcdef1234567890abcdef12345', true],
    ['ABC1234', 'abc1234567890abcdef1234567890abcdef12345', true],
    ['abc123', 'abc1234567890abcdef1234567890abcdef12345', false], // < 7 hex chars
    ['', 'abc1234567890abcdef1234567890abcdef12345', false],
    ['abc1234567890abcdef1234567890abcdef12345', 'abc1234567890abcdef1234567890abcdef12345', true],
    ['ghi5678', 'abc1234567890abcdef1234567890abcdef12345', false],
    ['abc1234x', 'abc1234567890abcdef1234567890abcdef12345', false], // non-hex
  ];

  // Helper-level sanity check on commitHashesAgree (symmetric prefix match).
  const agreeCases = [
    ['abc1234', 'abc1234', true],
    ['abc1234', 'abc1234567890', true], // short is prefix of long
    ['abc1234567890', 'abc1234', true], // long contains short prefix
    ['abc1234', 'deadbee', false],
    ['abc123', 'abc1234567890', false], // shorter than 7 hex
    ['', 'abc1234567890', false],
    ['abc1234', '', false],
    ['notHex!', 'abc1234567890', false],
  ];

  // wave29-04 helper-level sanity check on buildLineageEntries: confirms
  // the canonical role ordering (commit_hash → agent_commit_hash →
  // parent_patches[*].commit) plus de-duplication when the same hash
  // appears in multiple roles. Each case is [report-shape, expected-roles].
  const lineageCases = [
    [{ commitHash: 'abc1234' }, ['commit_hash']],
    [{ commitHash: 'abc1234', agentCommitHash: 'deadbee' }, ['commit_hash', 'agent_commit_hash']],
    [
      { commitHash: 'abc1234', agentCommitHash: 'deadbee', parentPatches: [{ commit: 'cafebab' }] },
      ['commit_hash', 'agent_commit_hash', 'parent_patch'],
    ],
    [
      // De-dup: when commit_hash and the trailing parent_patch share the
      // same hash, only the first occurrence (commit_hash) is emitted.
      { commitHash: 'abc1234', parentPatches: [{ commit: 'abc1234' }] },
      ['commit_hash'],
    ],
    [{}, []], // legacy report: no hashes at all → empty lineage
  ];

  const failures = [];
  for (const [reported, full, expected] of helperCases) {
    const got = commitHashMatches(reported, full);
    if (got !== expected) {
      failures.push({ kind: 'helper', case: `${reported} ~ ${full}`, expected, got });
    }
  }
  for (const [a, b, expected] of agreeCases) {
    const got = commitHashesAgree(a, b);
    if (got !== expected) {
      failures.push({ kind: 'helper-agree', case: `${a} ~ ${b}`, expected, got });
    }
  }
  for (const [report, expectedRoles] of lineageCases) {
    const got = buildLineageEntries(report).map((e) => e.role);
    const ok = got.length === expectedRoles.length && got.every((r, i) => r === expectedRoles[i]);
    if (!ok) {
      failures.push({
        kind: 'helper-lineage',
        case: JSON.stringify(report),
        expected: expectedRoles,
        got,
      });
    }
  }

  for (const fx of fixtures) {
    const result = verifyRun({
      contract: fx.contract,
      contractFile: fx.contractFile,
      report: fx.report,
      reportFile: fx.reportFile,
      ledger: fx.ledger,
      ledgerFile: fx.ledgerFile,
      ledgerStatus: fx.ledgerStatus,
      commitInfo: fx.commitInfo,
      trace: fx.trace ?? null,
      traceFile: fx.traceFile ?? null,
      traceStatus: fx.traceStatus ?? 'absent',
      traceLoadError: fx.traceLoadError ?? null,
      requireTrace: fx.requireTrace ?? false,
    });
    const okMatch = result.ok === fx.expectOk;
    let errMatch = true;
    let warnMatch = true;
    if (!fx.expectOk && fx.expectError) {
      errMatch = result.errors.some((e) => fx.expectError.test(e));
    }
    if (fx.expectWarning) {
      warnMatch = result.warnings.some((w) => fx.expectWarning.test(w));
    }
    if (!okMatch || !errMatch || !warnMatch) {
      failures.push({
        kind: 'fixture',
        name: fx.name,
        expected: {
          ok: fx.expectOk,
          errorMatches: fx.expectError?.toString(),
          warningMatches: fx.expectWarning?.toString(),
        },
        got: { ok: result.ok, errors: result.errors, warnings: result.warnings },
      });
    }
  }

  const ok = failures.length === 0;
  const totalHelpers = helperCases.length + agreeCases.length + lineageCases.length;
  if (json) {
    console.log(
      JSON.stringify(
        {
          ok,
          fixtures: fixtures.map((fx) => fx.name),
          helperCases: helperCases.length,
          agreeCases: agreeCases.length,
          lineageCases: lineageCases.length,
          failures,
        },
        null,
        2,
      ),
    );
  } else if (ok) {
    console.log(
      `task-run verify fixtures OK ` +
      `(${fixtures.length} fixture${fixtures.length === 1 ? '' : 's'}, ` +
      `${totalHelpers} helper case${totalHelpers === 1 ? '' : 's'})`,
    );
  } else {
    console.error(`task-run verify fixtures FAILED — ${failures.length} failure(s)`);
    for (const f of failures) console.error(JSON.stringify(f, null, 2));
  }
  process.exit(ok ? 0 : 1);
}

// --- Read-only invariant guard -------------------------------------------
//
// This module never invokes a mutating git verb directly. The list below is
// the canonical inventory of git verbs that this script must NOT call; the
// `assertReadOnlyGit` helper is exported so reviewers, tests, or future
// wrapper code can audit any candidate `git` argv against the same allowlist.
// Today the only git access here is via verify-task-contract.readCommit,
// which uses `git rev-parse | git log | git show` — all read-only.
const FORBIDDEN_GIT_VERBS = new Set([
  'add', 'commit', 'reset', 'checkout', 'restore', 'switch',
  'stash', 'push', 'merge', 'rebase', 'rm', 'mv', 'tag',
  'cherry-pick', 'revert', 'clean', 'fetch', 'pull',
]);

export function assertReadOnlyGit(args) {
  if (!Array.isArray(args) || args.length === 0) return;
  // The first non-flag argument is the git verb (e.g. `-C path` is a flag).
  let verb = null;
  for (const a of args) {
    if (typeof a !== 'string') continue;
    if (a.startsWith('-')) continue;
    verb = a;
    break;
  }
  if (verb && FORBIDDEN_GIT_VERBS.has(verb)) {
    throw new Error(
      `verify-task-run.mjs is read-only; refused mutating git verb "git ${verb}"`,
    );
  }
}

// Silence the linter: execFileSync is imported only to keep the symbol live
// in case future helpers need it; we never call it directly today.
void execFileSync;

// --- Entrypoint ----------------------------------------------------------

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.dryFixture) {
    runFixtures({ json: opts.json });
  } else {
    runCli(opts);
  }
}

export { commitHashMatches, commitHashesAgree, buildLineageEntries };
