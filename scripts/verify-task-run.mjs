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

const usage = `Usage:
  node scripts/verify-task-run.mjs \\
       --task <task.lisp> --report <report.lisp> \\
       --memory <shared-memory.lisp> --commit <hash> [--json]
  node scripts/verify-task-run.mjs --dry-fixture [--json]

Verifies a complete MissionD task run as a single post-run proof:
  1. task contract passes (delegates to verify-task-contract logic — message,
     :write-scope, :must-not-touch all checked against the actual commit)
  2. report :task_id matches the task contract id
  3. report :commit_hash matches the resolved commit (full or short prefix)
  4. shared-memory ledger contains a (completion ... :task <id>) entry that
     references the task

Flags:
  --task <task.lisp>            MissionD task-contract v1 file (required)
  --report <report.lisp>        MissionD report-contract v1 file (required)
  --memory <shared-memory.lisp> MissionD shared-memory v1 ledger (required
                                unless --allow-missing-memory is set)
  --commit <hash>               git ref to verify against; defaults to HEAD
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
  const filesChanged = nodeToStringArray(props[':files_changed']?.value);
  return { id, file, taskId, status, commitHash, filesChanged };
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

  // Check 3: report :commit_hash matches resolved commit (full or prefix).
  const commitMatch = commitHashMatches(report.commitHash, commitInfo.hash);
  checks.report_commit_hash = {
    ok: commitMatch,
    expected_full: commitInfo.hash,
    got: report.commitHash,
  };
  if (!commitMatch) {
    errors.push(
      `report :commit_hash mismatch — resolved commit is ${commitInfo.hash}, ` +
      `report has ${JSON.stringify(report.commitHash)}`,
    );
  }

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

  const result = verifyRun({
    contract,
    contractFile: taskFile,
    report,
    reportFile,
    ledger,
    ledgerFile,
    ledgerStatus,
    commitInfo,
  });

  emit(result, { json: opts.json });
  process.exit(result.ok ? 0 : 1);
}

// --- Fixtures -------------------------------------------------------------

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

  const failures = [];
  for (const [reported, full, expected] of helperCases) {
    const got = commitHashMatches(reported, full);
    if (got !== expected) {
      failures.push({ kind: 'helper', case: `${reported} ~ ${full}`, expected, got });
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
  if (json) {
    console.log(
      JSON.stringify(
        {
          ok,
          fixtures: fixtures.map((fx) => fx.name),
          helperCases: helperCases.length,
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
      `${helperCases.length} helper case${helperCases.length === 1 ? '' : 's'})`,
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

export { commitHashMatches };
