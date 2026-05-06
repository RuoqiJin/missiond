import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

export const TOOL_ROOT = 'tools/missiond_lispc';

export function toolchainStatus() {
  const commands = ['ocaml', 'dune', 'opam'];
  const found = Object.fromEntries(commands.map((cmd) => [cmd, commandExists(cmd)]));
  return {
    ok: commands.every((cmd) => found[cmd]),
    commands: found,
    missing: commands.filter((cmd) => !found[cmd]),
    install_hint:
      'Install OCaml tooling outside MissionD, for example: brew install ocaml opam dune. MissionD checkers never auto-install toolchains.',
  };
}

export function runLispc(args, { repoRoot = process.cwd(), timeoutMs = 60_000 } = {}) {
  const toolchain = toolchainStatus();
  if (!toolchain.ok) return unavailableResult(toolchain);

  const toolRoot = path.resolve(repoRoot, TOOL_ROOT);
  if (!fs.existsSync(path.join(toolRoot, 'dune-project'))) {
    return {
      ok: false,
      unavailable: true,
      toolchain,
      diagnostics: [diagnostic(TOOL_ROOT, 'OCAML_LISPC_MISSING', 'missing tools/missiond_lispc/dune-project')],
    };
  }

  const proc = spawnSync(
    'dune',
    ['exec', '--root', toolRoot, './bin/main.exe', '--', ...args],
    { cwd: repoRoot, encoding: 'utf8', timeout: timeoutMs },
  );
  const stdout = proc.stdout ?? '';
  const stderr = proc.stderr ?? '';
  let parsed = null;
  try {
    parsed = stdout.trim() ? JSON.parse(stdout) : null;
  } catch {
    parsed = null;
  }
  if (parsed && typeof parsed === 'object') {
    return {
      ...parsed,
      unavailable: false,
      toolchain,
      stdout,
      stderr,
      exit_code: proc.status,
      error: proc.error?.message ?? null,
    };
  }
  return {
    ok: proc.status === 0 && !proc.error,
    unavailable: false,
    toolchain,
    diagnostics: proc.status === 0 && !proc.error
      ? []
      : [diagnostic(TOOL_ROOT, 'OCAML_LISPC_FAILED', stderr.trim() || proc.error?.message || 'missiond-lispc failed without JSON output')],
    stdout,
    stderr,
    exit_code: proc.status,
    error: proc.error?.message ?? null,
  };
}

export function maybeRunLispc(args, { engine = 'auto', repoRoot = process.cwd(), timeoutMs = 60_000 } = {}) {
  if (engine === 'js') {
    return { mode: 'js', result: null };
  }
  if (!['auto', 'ocaml'].includes(engine)) {
    return {
      mode: 'invalid',
      result: {
        ok: false,
        diagnostics: [diagnostic('engine', 'ENGINE_INVALID', `unknown checker engine: ${engine}`)],
      },
    };
  }
  const result = runLispc(args, { repoRoot, timeoutMs });
  if (engine === 'ocaml') {
    return { mode: 'ocaml', result };
  }
  if (result.unavailable) {
    return { mode: 'js-fallback', result };
  }
  return { mode: 'ocaml', result };
}

function commandExists(cmd) {
  const proc = spawnSync('sh', ['-lc', `command -v ${quoteShell(cmd)} >/dev/null 2>&1`], {
    encoding: 'utf8',
  });
  return proc.status === 0;
}

function quoteShell(value) {
  return `'${String(value).replace(/'/g, `'\\''`)}'`;
}

function unavailableResult(toolchain) {
  return {
    ok: false,
    unavailable: true,
    toolchain,
    diagnostics: [
      diagnostic(
        TOOL_ROOT,
        'OCAML_TOOLCHAIN_MISSING',
        `missing OCaml toolchain command(s): ${toolchain.missing.join(', ')}. ${toolchain.install_hint}`,
      ),
    ],
  };
}

function diagnostic(file, code, message) {
  return {
    file,
    line: 1,
    column: 1,
    code,
    message,
    path: '',
  };
}
