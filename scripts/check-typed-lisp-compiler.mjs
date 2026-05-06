#!/usr/bin/env node

import fs from 'node:fs';
import { runLispc, toolchainStatus } from './lib/ocaml_lispc.mjs';
import { EXPECTED_SURFACES } from './check-v3-code-isomorphism-complete.mjs';

const BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const DEFAULT_AUTH_MISSIOND =
  '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/.missiond';

const REQUIRED_FILES = [
  'tools/missiond_lispc/dune-project',
  'tools/missiond_lispc/bin/dune',
  'tools/missiond_lispc/bin/main.ml',
  'tools/missiond_lispc/bin/ast.ml',
  'tools/missiond_lispc/bin/parser.ml',
  'tools/missiond_lispc/bin/schema_v3.ml',
  'tools/missiond_lispc/bin/workflow_schema.ml',
  'tools/missiond_lispc/bin/project_schema.ml',
  'tools/missiond_lispc/bin/emit_json.ml',
  'tools/missiond_lispc/test/dune',
  'tools/missiond_lispc/test/parser_golden.ml',
  'scripts/lib/ocaml_lispc.mjs',
  'scripts/check-ocaml-toolchain.mjs',
  'scripts/check-typed-lisp-compiler.mjs',
  'scripts/compile-v3-runtime.mjs',
  '.missiond/workflows/typed-lisp-compiler-convergence.lisp',
];

const REQUIRED_RUNTIME_LOADER = {
  file: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  tokens: [
    'load_runtime_blueprint_source',
    'load_compiled_v3_lisp_source',
    'compiled_v3_snapshot_is_current',
    'compiled_sexp_to_lisp',
    'CompiledV3Payload',
    'load_runtime_blueprint_source(project_root)',
    'CompiledProjectUniverse',
    'CompiledWorkflowContracts',
    'CompiledPayloadLoad',
    'load_compiled_project_universe',
    'load_compiled_workflow_contracts',
    'compiled_runtime_projection_status',
  ],
};

const REQUIRED_PROJECT_UNIVERSE_CHECKER = {
  file: 'scripts/check-project-ssot-universe.mjs',
  tokens: [
    'loadTypedUniverseProjects',
    "runLispc([",
    "'emit-universe'",
    'normalizeTypedUniversePayload',
    'PROJECT_CHECKERS',
    'OCaml owns project id/root/maturity facts',
    'compiled-project-universe.json',
  ],
};

const REQUIRED_PROJECT_MATURITY_CHECKER = {
  file: 'scripts/check-project-maturity.mjs',
  tokens: [
    'loadMaturityInputs',
    'normalizeTypedMaturityInputs',
    "'emit-universe'",
    'source: loaded.source',
    'mode: \'js-fixture\'',
  ],
};

const REQUIRED_AUTH_DOMAIN_CHECKER = {
  files: [
    {
      file: 'scripts/check-auth-domain-ssot.mjs',
      tokens: [
        "runLispc(['check-auth-domain'",
        '--dir',
        'DEFAULT_AUTH_MISSIOND',
      ],
    },
    {
      file: 'tools/missiond_lispc/bin/project_schema.ml',
      tokens: [
        'validate_auth_domain_structured_dir',
        'validate_auth_structured_form',
        'auth.runtime_projection_missing',
        'auth.shard_missing',
        '"runtime-registration-domain"',
        '"product-access-policy"',
        '"token-claim-contract"',
        '"outbox-delivery-state-machine"',
        '"legacy-callback-domain"',
      ],
    },
  ],
};

const REQUIRED_BLUEPRINT_TOKENS = [
  '(function typed-lisp-compiler',
  ':surface typed-lisp-compiler',
  '(surface typed-lisp-compiler',
  'tools/missiond_lispc/bin/main.ml',
  'tools/missiond_lispc/bin/schema_v3.ml',
  'tools/missiond_lispc/bin/emit_json.ml',
  'node scripts/check-typed-lisp-compiler.mjs',
];

const usage = `Usage:
  node scripts/check-typed-lisp-compiler.mjs [--json] [--strict-toolchain]

Checks that the OCaml typed Lisp compiler/checker layer is registered in V3.
Without --strict-toolchain, missing OCaml tooling is reported as a warning so
the existing JS gates can still run on machines that have not installed OCaml.
`;

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const diagnostics = [];
  const warnings = [];

  for (const file of REQUIRED_FILES) {
    if (!fs.existsSync(file)) diagnostics.push(diag(file, 'FILE_MISSING', 'required typed compiler file is missing'));
  }

  const runtimeLoader = read(REQUIRED_RUNTIME_LOADER.file);
  for (const token of REQUIRED_RUNTIME_LOADER.tokens) {
    if (!runtimeLoader.includes(token)) {
      diagnostics.push(diag(REQUIRED_RUNTIME_LOADER.file, 'RUNTIME_LOADER_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
    }
  }

  const universeChecker = read(REQUIRED_PROJECT_UNIVERSE_CHECKER.file);
  for (const token of REQUIRED_PROJECT_UNIVERSE_CHECKER.tokens) {
    if (!universeChecker.includes(token)) {
      diagnostics.push(diag(REQUIRED_PROJECT_UNIVERSE_CHECKER.file, 'UNIVERSE_CHECKER_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
    }
  }
  const maturityChecker = read(REQUIRED_PROJECT_MATURITY_CHECKER.file);
  for (const token of REQUIRED_PROJECT_MATURITY_CHECKER.tokens) {
    if (!maturityChecker.includes(token)) {
      diagnostics.push(diag(REQUIRED_PROJECT_MATURITY_CHECKER.file, 'MATURITY_CHECKER_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
    }
  }
  for (const spec of REQUIRED_AUTH_DOMAIN_CHECKER.files) {
    const source = read(spec.file);
    for (const token of spec.tokens) {
      if (!source.includes(token)) {
        diagnostics.push(diag(spec.file, 'AUTH_DOMAIN_CHECKER_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
      }
    }
  }

  const blueprint = read(BLUEPRINT);
  for (const token of REQUIRED_BLUEPRINT_TOKENS) {
    if (!blueprint.includes(token)) diagnostics.push(diag(BLUEPRINT, 'BLUEPRINT_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
  }

  const workflow = read('.missiond/workflows/typed-lisp-compiler-convergence.lisp');
  for (const token of [':workflow_id typed-lisp-compiler-convergence', ':status active', 'OCaml structural gate', 'compiled JSON']) {
    if (!workflow.includes(token)) diagnostics.push(diag('.missiond/workflows/typed-lisp-compiler-convergence.lisp', 'WORKFLOW_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
  }

  const toolchain = toolchainStatus();
  let ocaml = null;
  const emitChecks = [];
  if (toolchain.ok) {
    ocaml = runLispc([
      'check-v3',
      '--blueprint',
      BLUEPRINT,
      '--expected-surfaces',
      EXPECTED_SURFACES.join(','),
    ]);
    if (!ocaml.ok) {
      for (const d of ocaml.diagnostics ?? []) diagnostics.push({ ...d, code: d.code ?? 'OCAML_CHECK_FAILED' });
    }
    for (const argv of [
      ['emit-v3', '--blueprint', BLUEPRINT],
      ['emit-universe', '--blueprint', BLUEPRINT],
      ['emit-workflows', '--workflow-dir', '.missiond/workflows'],
    ]) {
      const emit = runLispc(argv);
      emitChecks.push({ argv, ok: emit.ok === true, diagnostics: emit.diagnostics ?? [] });
      if (!emit.ok || !emit.compiled) {
        diagnostics.push(diag('tools/missiond_lispc', 'OCAML_EMIT_FAILED', `emit command failed: ${argv.join(' ')}`));
      } else if (argv[0] === 'emit-universe') {
        if (!Array.isArray(emit.compiled?.payload?.projects) || emit.compiled.payload.projects.length === 0) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_UNIVERSE_PROJECTION_MISSING_PROJECTS', 'emit-universe must project structured projects[]'));
        }
        if (!Array.isArray(emit.compiled?.payload?.maturity) || emit.compiled.payload.maturity.length === 0) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_UNIVERSE_PROJECTION_MISSING_MATURITY', 'emit-universe must project structured maturity[]'));
        }
      } else if (argv[0] === 'emit-workflows') {
        if (!Array.isArray(emit.compiled?.payload?.workflows) || emit.compiled.payload.workflows.length === 0) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_WORKFLOW_PROJECTION_MISSING_WORKFLOWS', 'emit-workflows must project structured workflows[]'));
        }
        const domainHardening = emit.compiled?.payload?.workflows?.find((workflow) => workflow.name === 'project-domain-hardening');
        if (!Array.isArray(domainHardening?.steps) || !domainHardening.steps.includes('s10')) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_WORKFLOW_PROJECTION_MISSING_STRUCTURED_STEPS', 'emit-workflows must project step ids from structured (step sN ...) forms'));
        }
      }
    }
    if (fs.existsSync(DEFAULT_AUTH_MISSIOND)) {
      const authDomain = runLispc(['check-auth-domain', '--dir', DEFAULT_AUTH_MISSIOND]);
      emitChecks.push({
        argv: ['check-auth-domain', '--dir', DEFAULT_AUTH_MISSIOND],
        ok: authDomain.ok === true,
        diagnostics: authDomain.diagnostics ?? [],
      });
      if (!authDomain.ok) {
        diagnostics.push(diag('tools/missiond_lispc/bin/project_schema.ml', 'OCAML_AUTH_DOMAIN_GATE_FAILED', 'Auth domain structural sample gate failed'));
        for (const d of authDomain.diagnostics ?? []) diagnostics.push({ ...d, code: d.code ?? 'OCAML_AUTH_DOMAIN_DIAGNOSTIC' });
      }
    } else {
      warnings.push({
        file: DEFAULT_AUTH_MISSIOND,
        code: 'AUTH_DOMAIN_SAMPLE_UNAVAILABLE',
        message: 'Auth domain sample path not present on this machine; skipped external project structural gate.',
      });
    }
  } else if (opts.strictToolchain) {
    diagnostics.push(diag('tools/missiond_lispc', 'OCAML_TOOLCHAIN_MISSING', `missing OCaml command(s): ${toolchain.missing.join(', ')}`));
  } else {
    warnings.push({
      file: 'tools/missiond_lispc',
      code: 'OCAML_TOOLCHAIN_UNAVAILABLE',
      message: `OCaml strict gate skipped: missing ${toolchain.missing.join(', ')}. Existing JS gates remain authoritative on this host.`,
    });
  }

  const result = {
    ok: diagnostics.length === 0,
    toolchain,
    ocaml_strict_ran: toolchain.ok,
    diagnostics,
    warnings,
    ocaml,
    emit_checks: emitChecks,
  };
  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('typed Lisp compiler registration OK');
    for (const w of warnings) console.log(`warning: ${w.message}`);
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.code}: ${d.message}`);
    console.error('typed Lisp compiler registration FAILED');
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, strictToolchain: false };
  for (const arg of argv) {
    if (arg === '--json') opts.json = true;
    else if (arg === '--strict-toolchain') opts.strictToolchain = true;
    else if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else {
      console.error(`unknown argument: ${arg}\n\n${usage}`);
      process.exit(2);
    }
  }
  return opts;
}

function read(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return '';
  }
}

function diag(file, code, message) {
  return { file, line: 1, column: 1, code, message, path: '' };
}

main();
