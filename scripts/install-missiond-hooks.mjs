#!/usr/bin/env node

import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// MissionD hooksPath installer v2 (default-on doctor).
//
// Repo-local doctor + installer for core.hooksPath = .githooks. Two side-by-side
// concerns:
//   1) Default-on doctor: `--check` (the implicit default mode) reads git config
//      core.hooksPath and reports whether the .githooks/ pre-commit hook is the
//      active one for this clone. Drift is treated as a *preflight problem* —
//      the JSON payload carries severity/advice with a concrete install command
//      so callers (renderer, agents, CI) can surface it without mutating git
//      config themselves. Exit code is 0 by default; `--strict` makes drift a
//      hard non-zero exit. The doctor never mutates anything.
//   2) Explicit installer: `--install` runs `git config --local
//      core.hooksPath .githooks` exactly once and never touches --global or
//      --system. This is the ONLY surface in this script that mutates state.
//
// Default-on note: invoking the script without a mode is equivalent to
// `--check`. This makes the doctor the default-on preflight expectation while
// keeping all mutation explicit and opt-in (an unattended invocation will
// never silently flip git config). The renderer surfaces the doctor commands
// as preflight steps in commit-required task briefs so dispatched agents see
// the expectation up front.
//
// The installer is intentionally repo-local. It never touches --global /
// --system git config. It never enables hooks for any other clone. It never
// writes any file outside the repo's .git/config (via `git config`).
//
// Modes:
//   --check          read-only doctor (DEFAULT when no mode flag is given);
//                    prints state + advice. Exits 0 by default even on drift
//                    so callers decide how to react; pair with --strict to
//                    make drift a hard non-zero exit.
//   --install        runs `git config --local core.hooksPath .githooks`
//                    exactly once. no-op + exit 0 if already set to the
//                    expected value.
//   --json           machine-readable output for either mode.
//   --dry-fixture    self-contained fixture run; no git invoked, no disk
//                    writes; exits non-zero on any fixture failure.
//   --strict         only meaningful with --check: makes drift a hard error.
//
// This script is read-only in --check mode and write-once in --install mode
// (the single mutation is `git config --local core.hooksPath .githooks`). It
// NEVER runs git add / commit / reset / checkout / stash / push / merge /
// rebase, and NEVER touches --global or --system git config.

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const SCRIPT_DIR = path.dirname(SCRIPT_PATH);
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..');
const EXPECTED_HOOKS_PATH = '.githooks';
const EXPECTED_HOOK_FILE = path.join(REPO_ROOT, EXPECTED_HOOKS_PATH, 'pre-commit');

const usage = `Usage:
  node scripts/install-missiond-hooks.mjs [--check] [--json] [--strict]
  node scripts/install-missiond-hooks.mjs --install [--json]
  node scripts/install-missiond-hooks.mjs --dry-fixture [--json]

Repo-local installer + doctor for git core.hooksPath = .githooks.

Default mode (no flag) is --check: a read-only preflight doctor that surfaces
unset / wrong core.hooksPath as a preflight problem with a concrete install
command, but never mutates git config on its own.

Modes:
  --check        Read git config --get core.hooksPath and report whether it
                 equals "${EXPECTED_HOOKS_PATH}". Also reports whether the
                 expected ${EXPECTED_HOOKS_PATH}/pre-commit file exists. The
                 JSON payload includes a severity tag (ok | preflight-drift)
                 and a concrete advice command. By default exits 0 even on
                 drift; pair with --strict to make drift a hard non-zero exit
                 (useful for CI). DEFAULT mode when no flag is supplied.
  --install      Run \`git config --local core.hooksPath ${EXPECTED_HOOKS_PATH}\`.
                 No-op when the value is already set. Local repo config only;
                 never touches --global or --system. Performs no other
                 mutations.
  --dry-fixture  Self-contained fixtures (no git invoked, no disk writes)
                 covering installed / unset / wrong-path / missing-hook-file
                 states plus the install state machine. Exits non-zero on
                 any fixture failure.

Flags:
  --json         Machine-readable JSON output.
  --strict       --check exits non-zero when core.hooksPath != "${EXPECTED_HOOKS_PATH}"
                 or the hook file is missing.

The installer is repo-local. It never enables hooks globally and never runs
mutating git commands beyond the single \`git config --local core.hooksPath\`
write. The doctor (default mode) NEVER mutates git config; only --install does.
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

  // Default-on doctor: invoking the script with no mode flag falls through to
  // --check. This makes the doctor the implicit preflight expectation while
  // keeping every mutation explicit (--install must be opted into).
  if (!mode) mode = '--check';

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

// Doctor severity tags surface in the JSON payload so callers (renderer,
// agents, CI) can react to the preflight state without parsing prose.
//   ok                 — core.hooksPath aligned + hook file present
//   preflight-drift    — alignment problem the user can resolve with --install
//                        (covers unset, wrong path, and missing hook file
//                        states; the advice command differs per state)
export const DOCTOR_SEVERITY = Object.freeze({
  OK: 'ok',
  PREFLIGHT_DRIFT: 'preflight-drift',
});

// Classify the doctor state into a discrete reason code so machine consumers
// (renderer, planners, CI) can branch without re-implementing the matrix.
//   aligned            — core.hooksPath==expected AND hook file present
//   hooks-path-unset   — git config core.hooksPath returned no value
//   hooks-path-wrong   — git config core.hooksPath != expected (set elsewhere)
//   hook-file-missing  — expected hook file absent from working tree
function classifyDoctorState(info) {
  if (!info.hook_file_exists) return 'hook-file-missing';
  if (info.current_hooks_path == null) return 'hooks-path-unset';
  if (!info.matches) return 'hooks-path-wrong';
  return 'aligned';
}

export function inspectHooksPath({ git = realGit } = {}) {
  const repoRoot = git.repoRoot();
  const expectedHooksDir = path.join(repoRoot, EXPECTED_HOOKS_PATH);
  const expectedHookFile = path.join(expectedHooksDir, 'pre-commit');
  const current = git.getCoreHooksPath();
  const matches = current === EXPECTED_HOOKS_PATH;
  const hookFileExists = fs.existsSync(expectedHookFile);
  const hookFileExecutable = hookFileExists ? isExecutable(expectedHookFile) : false;
  const info = {
    repo_root: repoRoot,
    expected_hooks_path: EXPECTED_HOOKS_PATH,
    current_hooks_path: current,
    matches,
    expected_hook_file: path.relative(repoRoot, expectedHookFile),
    hook_file_exists: hookFileExists,
    hook_file_executable: hookFileExecutable,
  };
  info.reason = classifyDoctorState(info);
  return info;
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
        severity: DOCTOR_SEVERITY.PREFLIGHT_DRIFT,
        error: `failed to read git state: ${err.message ?? err}`,
      },
      humanFail: (p) => `install-missiond-hooks check FAILED: ${p.error}`,
    });
    process.exit(1);
  }

  const ok = info.matches && info.hook_file_exists;
  const severity = ok ? DOCTOR_SEVERITY.OK : DOCTOR_SEVERITY.PREFLIGHT_DRIFT;
  const payload = {
    mode: 'check',
    ok,
    severity,
    preflight: 'core.hooksPath',
    strict,
    ...info,
    advice: ok ? null : adviceFor(info),
    install_command: ok ? null : 'node scripts/install-missiond-hooks.mjs --install',
  };

  emit({
    json,
    payload,
    humanOk: (p) =>
      `install-missiond-hooks check OK: core.hooksPath=${p.current_hooks_path} (expected ${p.expected_hooks_path}); ` +
      `hook file ${p.expected_hook_file} present` +
      (p.hook_file_executable ? '' : ' (not marked executable; git still runs it via /bin/sh)'),
    humanFail: (p) =>
      `install-missiond-hooks check PREFLIGHT-DRIFT [${p.reason}]: ` +
      `core.hooksPath=${p.current_hooks_path ?? '<unset>'} ` +
      `(expected ${p.expected_hooks_path}); ` +
      `hook file ${p.expected_hook_file} ${p.hook_file_exists ? 'present' : 'MISSING'}\n  ${p.advice}`,
  });

  if (!ok && strict) process.exit(1);
  process.exit(0);
}

function adviceFor(info) {
  if (!info.hook_file_exists) {
    return `Hook file ${info.expected_hook_file} is missing — restore it before enabling core.hooksPath. Do not run --install while the hook file is absent; the installer refuses in that state.`;
  }
  if (info.current_hooks_path == null) {
    return `core.hooksPath is unset for this clone. Run \`node scripts/install-missiond-hooks.mjs --install\` (repo-local mutation only) to opt this clone in.`;
  }
  if (!info.matches) {
    return `core.hooksPath is set to ${info.current_hooks_path}, expected ${info.expected_hooks_path}. Run \`node scripts/install-missiond-hooks.mjs --install\` to switch this clone over (writes --local config only).`;
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

function makeFakeGit({ hooksPath, repoRoot = REPO_ROOT }) {
  let stored = hooksPath;
  return {
    repoRoot() {
      return repoRoot;
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
  };
}

// Build a fake git adapter rooted at a temp directory so we can simulate the
// "missing hook file" doctor state without touching the real working tree.
// No disk writes: we simply point the adapter at a path where .githooks/
// does not exist. fs.existsSync returns false there, exercising the
// hook-file-missing branch deterministically.
function makeFakeGitWithMissingHookFile({ hooksPath = EXPECTED_HOOKS_PATH } = {}) {
  // Choose a path that is virtually guaranteed to be absent so the test
  // remains hermetic. We never create or write to it.
  const fakeRoot = path.join(REPO_ROOT, '__missiond_dry_fixture_root_does_not_exist__');
  return makeFakeGit({ hooksPath, repoRoot: fakeRoot });
}

function runFixtures({ json }) {
  const failures = [];

  // The four required dry-fixture doctor states. Each fixture exercises
  // inspectHooksPath against a fake git adapter and asserts the resulting
  // reason code + matches flag, so the doctor surface stays mechanically
  // verifiable without invoking real git.

  // 1. doctor state: installed (matches + hook file present) -> reason 'aligned'
  {
    const git = makeFakeGit({ hooksPath: EXPECTED_HOOKS_PATH });
    const info = inspectHooksPath({ git });
    if (!(info.matches && info.hook_file_exists && info.reason === 'aligned')) {
      failures.push({
        name: 'doctor state: installed (aligned)',
        expected: { matches: true, hook_file_exists: true, reason: 'aligned' },
        got: { matches: info.matches, hook_file_exists: info.hook_file_exists, reason: info.reason },
      });
    }
  }

  // 2. doctor state: unset core.hooksPath -> reason 'hooks-path-unset'
  {
    const git = makeFakeGit({ hooksPath: null });
    const info = inspectHooksPath({ git });
    if (
      info.matches !== false ||
      info.current_hooks_path !== null ||
      info.reason !== 'hooks-path-unset'
    ) {
      failures.push({
        name: 'doctor state: unset core.hooksPath',
        expected: { matches: false, current_hooks_path: null, reason: 'hooks-path-unset' },
        got: {
          matches: info.matches,
          current_hooks_path: info.current_hooks_path,
          reason: info.reason,
        },
      });
    }
  }

  // 3. doctor state: wrong path (set elsewhere) -> reason 'hooks-path-wrong'
  {
    const git = makeFakeGit({ hooksPath: '.git/hooks' });
    const info = inspectHooksPath({ git });
    if (
      info.matches !== false ||
      info.current_hooks_path !== '.git/hooks' ||
      info.reason !== 'hooks-path-wrong'
    ) {
      failures.push({
        name: 'doctor state: wrong core.hooksPath',
        expected: { matches: false, current_hooks_path: '.git/hooks', reason: 'hooks-path-wrong' },
        got: {
          matches: info.matches,
          current_hooks_path: info.current_hooks_path,
          reason: info.reason,
        },
      });
    }
  }

  // 4. doctor state: missing hook file -> reason 'hook-file-missing' (takes
  //    priority over hooks-path alignment because fixing the path while the
  //    hook file is absent would just silently disable hooks)
  {
    const git = makeFakeGitWithMissingHookFile({ hooksPath: EXPECTED_HOOKS_PATH });
    const info = inspectHooksPath({ git });
    if (info.hook_file_exists !== false || info.reason !== 'hook-file-missing') {
      failures.push({
        name: 'doctor state: missing hook file',
        expected: { hook_file_exists: false, reason: 'hook-file-missing' },
        got: { hook_file_exists: info.hook_file_exists, reason: info.reason },
      });
    }
  }

  // 5. install: from drift -> mutates exactly once and reports change
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

  // 6. install: from already-aligned -> no-op, ok=true changed=false
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

  // 7. install: from unset -> sets to .githooks
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

  // 8. install: refuses when hook file is missing (must not silently arm a
  //    no-op hooksPath; doctor advice already says so).
  {
    const git = makeFakeGitWithMissingHookFile({ hooksPath: null });
    const result = performInstall({ git });
    if (result.ok !== false || result.changed !== false || git._peek() !== null) {
      failures.push({
        name: 'install: refuses when hook file missing',
        expected: { ok: false, changed: false, finalValue: null },
        got: { ok: result.ok, changed: result.changed, finalValue: git._peek() },
      });
    }
  }

  // 9. install: never touches global/system. The fake git has no
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

  // 10. doctor adviceFor() emits the expected install-command shape for each
  //     non-ok reason so the renderer / agents can rely on the surface text.
  {
    const adviceUnset = adviceFor({
      hook_file_exists: true,
      current_hooks_path: null,
      matches: false,
      expected_hooks_path: EXPECTED_HOOKS_PATH,
      expected_hook_file: '.githooks/pre-commit',
    });
    const adviceWrong = adviceFor({
      hook_file_exists: true,
      current_hooks_path: '.git/hooks',
      matches: false,
      expected_hooks_path: EXPECTED_HOOKS_PATH,
      expected_hook_file: '.githooks/pre-commit',
    });
    const adviceMissing = adviceFor({
      hook_file_exists: false,
      current_hooks_path: EXPECTED_HOOKS_PATH,
      matches: true,
      expected_hooks_path: EXPECTED_HOOKS_PATH,
      expected_hook_file: '.githooks/pre-commit',
    });
    const wantInstallSnippet = 'node scripts/install-missiond-hooks.mjs --install';
    const adviceOk =
      typeof adviceUnset === 'string' && adviceUnset.includes(wantInstallSnippet) &&
      typeof adviceWrong === 'string' && adviceWrong.includes(wantInstallSnippet) &&
      typeof adviceMissing === 'string' && adviceMissing.includes('Hook file');
    if (!adviceOk) {
      failures.push({
        name: 'doctor advice surfaces install command for each drift state',
        expected: {
          unset_includes: wantInstallSnippet,
          wrong_includes: wantInstallSnippet,
          missing_mentions_hook_file: true,
        },
        got: { adviceUnset, adviceWrong, adviceMissing },
      });
    }
  }

  // 11. fixture sanity: the real .githooks/pre-commit file ships in the repo
  {
    if (!fs.existsSync(EXPECTED_HOOK_FILE)) {
      failures.push({
        name: 'repo ships .githooks/pre-commit',
        expected: { exists: true, path: path.relative(REPO_ROOT, EXPECTED_HOOK_FILE) },
        got: { exists: false },
      });
    }
  }

  const FIXTURES_RUN = 11;
  const ok = failures.length === 0;
  const summary = {
    mode: 'dry-fixture',
    ok,
    fixtures_run: FIXTURES_RUN,
    doctor_states_covered: ['installed', 'unset', 'wrong-path', 'missing-hook-file'],
    failures,
  };

  if (json) {
    console.log(JSON.stringify(summary, null, 2));
  } else if (ok) {
    console.log(
      `install-missiond-hooks fixtures OK (${FIXTURES_RUN}/${FIXTURES_RUN}) — doctor states: installed, unset, wrong-path, missing-hook-file`,
    );
  } else {
    console.error(`install-missiond-hooks fixtures FAILED — ${failures.length} failure(s)`);
    for (const f of failures) console.error(JSON.stringify(f, null, 2));
  }
  process.exit(ok ? 0 : 1);
}

main();
