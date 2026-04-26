#!/usr/bin/env node

import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// MissionD hooksPath installer v1.
//
// Repo-local doctor + installer for core.hooksPath = .githooks. Two side-by-side
// concerns:
//   1) Read git config core.hooksPath and report whether the .githooks/
//      pre-commit hook is the active one for this clone.
//   2) Optionally run `git config core.hooksPath .githooks` (LOCAL repo config
//      only) so the next commit picks up the MissionD task-scope-guard.
//
// The installer is intentionally repo-local. It never touches --global /
// --system git config. It never enables hooks for any other clone. It never
// writes any file outside the repo's .git/config (via `git config`).
//
// Modes:
//   --check          read-only doctor; prints state + exits non-zero on drift
//                    when --strict is also passed (default exit 0 with
//                    state in stdout so the caller decides).
//   --install        runs `git config core.hooksPath .githooks` exactly once.
//                    no-op + exit 0 if already set to the expected value.
//   --json           machine-readable output for either mode.
//   --dry-fixture    self-contained fixture run; no git invoked, no disk
//                    writes; exits non-zero on any fixture failure.
//   --strict         only meaningful with --check: makes drift a hard error.
//
// This script is read-only in --check mode and write-once in --install mode
// (the single mutation is `git config core.hooksPath .githooks`). It NEVER
// runs git add / commit / reset / checkout / stash / push / merge / rebase.

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const SCRIPT_DIR = path.dirname(SCRIPT_PATH);
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..');
const EXPECTED_HOOKS_PATH = '.githooks';
const EXPECTED_HOOK_FILE = path.join(REPO_ROOT, EXPECTED_HOOKS_PATH, 'pre-commit');

const usage = `Usage:
  node scripts/install-missiond-hooks.mjs --check [--json] [--strict]
  node scripts/install-missiond-hooks.mjs --install [--json]
  node scripts/install-missiond-hooks.mjs --dry-fixture [--json]

Repo-local installer + doctor for git core.hooksPath = .githooks.

Modes:
  --check        Read git config --get core.hooksPath and report whether it
                 equals "${EXPECTED_HOOKS_PATH}". Also reports whether the
                 expected ${EXPECTED_HOOKS_PATH}/pre-commit file exists. By
                 default exits 0 even on drift; pair with --strict to make
                 drift a hard non-zero exit (useful for CI).
  --install      Run \`git config core.hooksPath ${EXPECTED_HOOKS_PATH}\`.
                 No-op when the value is already set. Local repo config only;
                 never touches --global or --system. Performs no other
                 mutations.
  --dry-fixture  Self-contained fixtures (no git invoked, no disk writes).
                 Exits non-zero on any fixture failure.

Flags:
  --json         Machine-readable JSON output.
  --strict       --check exits non-zero when core.hooksPath != "${EXPECTED_HOOKS_PATH}"
                 or the hook file is missing.

The installer is repo-local. It never enables hooks globally and never runs
mutating git commands beyond the single \`git config core.hooksPath\` write.
`;

function failUsage(message) {
  process.stderr.write(`error: ${message}\n\n${usage}`);
  process.exit(2);
}

function main() {
  const argv = process.argv.slice(2);
  let mode = null;
  let json = false;
  let strict = false;

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--strict') {
      strict = true;
    } else if (arg === '--check' || arg === '--install' || arg === '--dry-fixture') {
      if (mode && mode !== arg) {
        failUsage(`conflicting modes: --${mode} and ${arg}`);
      }
      mode = arg;
    } else if (arg.startsWith('--')) {
      failUsage(`unknown flag: ${arg}`);
    } else {
      failUsage(`unexpected positional argument: ${arg}`);
    }
  }

  if (!mode) failUsage('one of --check | --install | --dry-fixture is required');

  if (mode === '--dry-fixture') {
    runFixtures({ json });
    return;
  }

  if (mode === '--check') {
    runCheck({ json, strict });
    return;
  }

  runInstall({ json });
}

// --- check (read-only doctor) ---------------------------------------------

export function inspectHooksPath({ git = realGit } = {}) {
  const repoRoot = git.repoRoot();
  const expectedHooksDir = path.join(repoRoot, EXPECTED_HOOKS_PATH);
  const expectedHookFile = path.join(expectedHooksDir, 'pre-commit');
  const current = git.getCoreHooksPath();
  const matches = current === EXPECTED_HOOKS_PATH;
  const hookFileExists = fs.existsSync(expectedHookFile);
  const hookFileExecutable = hookFileExists ? isExecutable(expectedHookFile) : false;
  return {
    repo_root: repoRoot,
    expected_hooks_path: EXPECTED_HOOKS_PATH,
    current_hooks_path: current,
    matches,
    expected_hook_file: path.relative(repoRoot, expectedHookFile),
    hook_file_exists: hookFileExists,
    hook_file_executable: hookFileExecutable,
  };
}

function runCheck({ json, strict }) {
  let info;
  try {
    info = inspectHooksPath();
  } catch (err) {
    emit({
      json,
      payload: {
        mode: 'check',
        ok: false,
        error: `failed to read git state: ${err.message ?? err}`,
      },
      humanFail: (p) => `install-missiond-hooks check FAILED: ${p.error}`,
    });
    process.exit(1);
  }

  const ok = info.matches && info.hook_file_exists;
  const payload = {
    mode: 'check',
    ok,
    strict,
    ...info,
    advice: ok
      ? null
      : adviceFor(info),
  };

  emit({
    json,
    payload,
    humanOk: (p) =>
      `install-missiond-hooks check OK: core.hooksPath=${p.current_hooks_path} (expected ${p.expected_hooks_path}); ` +
      `hook file ${p.expected_hook_file} present` +
      (p.hook_file_executable ? '' : ' (not marked executable; git still runs it via /bin/sh)'),
    humanFail: (p) =>
      `install-missiond-hooks check DRIFT: core.hooksPath=${p.current_hooks_path ?? '<unset>'} ` +
      `(expected ${p.expected_hooks_path}); ` +
      `hook file ${p.expected_hook_file} ${p.hook_file_exists ? 'present' : 'MISSING'}\n  ${p.advice}`,
  });

  if (!ok && strict) process.exit(1);
  process.exit(0);
}

function adviceFor(info) {
  if (!info.hook_file_exists) {
    return `Hook file ${info.expected_hook_file} is missing — restore it before enabling core.hooksPath.`;
  }
  if (!info.matches) {
    return `Run \`node scripts/install-missiond-hooks.mjs --install\` (or \`git config core.hooksPath ${info.expected_hooks_path}\`) to opt this clone in.`;
  }
  return null;
}

// --- install (write-once mutation) ----------------------------------------

export function performInstall({ git = realGit } = {}) {
  const before = inspectHooksPath({ git });
  if (!before.hook_file_exists) {
    return {
      ok: false,
      changed: false,
      reason: `cannot install: ${before.expected_hook_file} is missing in the working tree`,
      before,
      after: before,
    };
  }
  if (before.matches) {
    return {
      ok: true,
      changed: false,
      reason: `core.hooksPath already set to ${before.expected_hooks_path}; nothing to do`,
      before,
      after: before,
    };
  }
  git.setCoreHooksPath(EXPECTED_HOOKS_PATH);
  const after = inspectHooksPath({ git });
  return {
    ok: after.matches,
    changed: true,
    reason: after.matches
      ? `set core.hooksPath ${before.current_hooks_path ?? '<unset>'} -> ${after.current_hooks_path}`
      : `git accepted the write but core.hooksPath now reads ${after.current_hooks_path ?? '<unset>'}`,
    before,
    after,
  };
}

function runInstall({ json }) {
  let result;
  try {
    result = performInstall();
  } catch (err) {
    emit({
      json,
      payload: {
        mode: 'install',
        ok: false,
        error: `failed to invoke git: ${err.message ?? err}`,
      },
      humanFail: (p) => `install-missiond-hooks install FAILED: ${p.error}`,
    });
    process.exit(1);
  }

  const payload = {
    mode: 'install',
    ok: result.ok,
    changed: result.changed,
    reason: result.reason,
    before: result.before,
    after: result.after,
  };

  emit({
    json,
    payload,
    humanOk: (p) =>
      p.changed
        ? `install-missiond-hooks install OK: ${p.reason}`
        : `install-missiond-hooks install OK (no-op): ${p.reason}`,
    humanFail: (p) => `install-missiond-hooks install FAILED: ${p.reason}`,
  });
  process.exit(result.ok ? 0 : 1);
}

// --- git adapter (real + injectable) --------------------------------------

const realGit = {
  repoRoot() {
    const out = execFileSync('git', ['rev-parse', '--show-toplevel'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    return out.trim();
  },
  getCoreHooksPath() {
    const child = spawnSync('git', ['config', '--local', '--get', 'core.hooksPath'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    if (child.status === 0) return child.stdout.trim() || null;
    if (child.status === 1) return null;
    if (child.error) throw child.error;
    throw new Error(
      `git config --local --get core.hooksPath exited ${child.status}: ${child.stderr.trim()}`,
    );
  },
  setCoreHooksPath(value) {
    execFileSync('git', ['config', '--local', 'core.hooksPath', value], {
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  },
};

function isExecutable(file) {
  try {
    fs.accessSync(file, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

// --- output ---------------------------------------------------------------

function emit({ json, payload, humanOk, humanFail }) {
  if (json) {
    console.log(JSON.stringify(payload, null, 2));
    return;
  }
  if (payload.ok) {
    console.log(humanOk ? humanOk(payload) : `install-missiond-hooks ${payload.mode} OK`);
  } else {
    console.error(
      humanFail
        ? humanFail(payload)
        : `install-missiond-hooks ${payload.mode} FAILED`,
    );
  }
}

// --- fixtures -------------------------------------------------------------

function makeFakeGit({ hooksPath, hookFileExists = true }) {
  let stored = hooksPath;
  return {
    repoRoot() {
      return REPO_ROOT;
    },
    getCoreHooksPath() {
      return stored;
    },
    setCoreHooksPath(value) {
      stored = value;
    },
    _peek() {
      return stored;
    },
    _hookFileExists: hookFileExists,
  };
}

function runFixtures({ json }) {
  const failures = [];

  // 1. inspect: matches + hook file exists -> ok
  {
    const git = makeFakeGit({ hooksPath: EXPECTED_HOOKS_PATH });
    const info = inspectHooksPath({ git });
    if (!(info.matches && info.hook_file_exists)) {
      failures.push({
        name: 'inspect: matches + hook file present',
        expected: { matches: true, hook_file_exists: true },
        got: { matches: info.matches, hook_file_exists: info.hook_file_exists },
      });
    }
  }

  // 2. inspect: unset core.hooksPath -> drift
  {
    const git = makeFakeGit({ hooksPath: null });
    const info = inspectHooksPath({ git });
    if (info.matches !== false || info.current_hooks_path !== null) {
      failures.push({
        name: 'inspect: unset core.hooksPath reports drift',
        expected: { matches: false, current_hooks_path: null },
        got: { matches: info.matches, current_hooks_path: info.current_hooks_path },
      });
    }
  }

  // 3. inspect: wrong value reports drift with current value preserved
  {
    const git = makeFakeGit({ hooksPath: '.git/hooks' });
    const info = inspectHooksPath({ git });
    if (info.matches !== false || info.current_hooks_path !== '.git/hooks') {
      failures.push({
        name: 'inspect: wrong core.hooksPath reports drift',
        expected: { matches: false, current_hooks_path: '.git/hooks' },
        got: { matches: info.matches, current_hooks_path: info.current_hooks_path },
      });
    }
  }

  // 4. install: from drift -> mutates exactly once and reports change
  {
    const git = makeFakeGit({ hooksPath: '.git/hooks' });
    const result = performInstall({ git });
    if (!(result.ok && result.changed && git._peek() === EXPECTED_HOOKS_PATH)) {
      failures.push({
        name: 'install: drift -> sets core.hooksPath to .githooks',
        expected: { ok: true, changed: true, finalValue: EXPECTED_HOOKS_PATH },
        got: { ok: result.ok, changed: result.changed, finalValue: git._peek() },
      });
    }
  }

  // 5. install: from already-aligned -> no-op, ok=true changed=false
  {
    const git = makeFakeGit({ hooksPath: EXPECTED_HOOKS_PATH });
    const result = performInstall({ git });
    if (!(result.ok && result.changed === false)) {
      failures.push({
        name: 'install: already aligned -> no-op',
        expected: { ok: true, changed: false },
        got: { ok: result.ok, changed: result.changed },
      });
    }
  }

  // 6. install: from unset -> sets to .githooks
  {
    const git = makeFakeGit({ hooksPath: null });
    const result = performInstall({ git });
    if (!(result.ok && result.changed && git._peek() === EXPECTED_HOOKS_PATH)) {
      failures.push({
        name: 'install: unset -> sets core.hooksPath to .githooks',
        expected: { ok: true, changed: true, finalValue: EXPECTED_HOOKS_PATH },
        got: { ok: result.ok, changed: result.changed, finalValue: git._peek() },
      });
    }
  }

  // 7. install: never touches global/system. The fake git has no
  //    global/system surface; the real adapter passes --local explicitly.
  //    Here we assert the adapter signature: realGit.setCoreHooksPath uses
  //    --local form. Read the source string for the constant we control.
  {
    const adapterSrc = realGit.setCoreHooksPath.toString();
    if (!/--local/.test(adapterSrc)) {
      failures.push({
        name: 'install adapter passes --local to git config',
        expected: { adapterIncludes: '--local' },
        got: { adapterSrc },
      });
    }
  }

  // 8. fixture sanity: the real .githooks/pre-commit file ships in the repo
  {
    if (!fs.existsSync(EXPECTED_HOOK_FILE)) {
      failures.push({
        name: 'repo ships .githooks/pre-commit',
        expected: { exists: true, path: path.relative(REPO_ROOT, EXPECTED_HOOK_FILE) },
        got: { exists: false },
      });
    }
  }

  const ok = failures.length === 0;
  const summary = {
    mode: 'dry-fixture',
    ok,
    fixtures_run: 8,
    failures,
  };

  if (json) {
    console.log(JSON.stringify(summary, null, 2));
  } else if (ok) {
    console.log('install-missiond-hooks fixtures OK (8/8)');
  } else {
    console.error(`install-missiond-hooks fixtures FAILED — ${failures.length} failure(s)`);
    for (const f of failures) console.error(JSON.stringify(f, null, 2));
  }
  process.exit(ok ? 0 : 1);
}

main();
