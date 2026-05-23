#!/usr/bin/env node

import fs from 'node:fs';
import { runLispc, toolchainStatus } from './lib/ocaml_lispc.mjs';
import { EXPECTED_SURFACES } from './check-v3-code-isomorphism-complete.mjs';
import {
  compiledSurfaceIds,
  loadCompiledV3Contract,
} from './lib/v3_compiled_contract.mjs';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

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
  'tools/missiond_lispc/bin/workstation_schema.ml',
  'tools/missiond_lispc/bin/genome_schema.ml',
  'tools/missiond_lispc/bin/emit_json.ml',
  'tools/missiond_lispc/test/dune',
  'tools/missiond_lispc/test/parser_golden.ml',
  'scripts/lib/ocaml_lispc.mjs',
  'scripts/lib/v3_compiled_contract.mjs',
  'scripts/check-ocaml-toolchain.mjs',
  'scripts/check-typed-lisp-compiler.mjs',
  'scripts/compile-v3-runtime.mjs',
  'scripts/project-v3-contracts.mjs',
  'scripts/generated/v3_contracts.mjs',
  'scripts/generated/v3_contracts.d.ts',
  'crates/missiond-daemon/src/context/v3_contracts/mod.rs',
  'crates/missiond-daemon/src/context/v3_contracts/generated.rs',
  'crates/missiond-daemon/src/context/v3_blueprint_runtime/compiled_envelope.rs',
  'crates/missiond-daemon/src/context/v3_blueprint_runtime/source_fallback.rs',
  'scripts/check-project-domain-hardening.mjs',
  '.missiond/workflows/typed-lisp-compiler-convergence.lisp',
  '.missiond/workflows/typed-lisp-compiler-cleanup.lisp',
  '.missiond/workflows/multi-project-m6-wave.lisp',
  '.missiond/workflows/project-m6-depth.lisp',
];

const REQUIRED_RUNTIME_LOADER = {
  file: 'crates/missiond-daemon/src/context/v3_blueprint_runtime.rs',
  tokens: [
    'load_runtime_blueprint_source',
    'load_runtime_blueprint_source_with_diagnostics',
    'load_compiled_v3_lisp_source',
    'load_compiled_v3_lisp_source_with_diagnostics',
    'load_compiled_runtime_config',
    'required_compiled_runtime_config',
    'source_fallback::allowed()',
    'CompiledRuntimeConfigPayload',
    'CompiledSourceUnit',
    'validate_compiled_source_units',
    'md5_hex',
    'compiled_sexp_to_lisp',
    'CompiledV3Payload',
    'RuntimeBlueprintSourceKind',
    'load_runtime_blueprint_source(project_root)',
    'CompiledProjectUniverse',
    'CompiledWorkflowContracts',
    'CompiledPayloadLoad',
    'load_compiled_project_universe',
    'load_compiled_workflow_contracts',
    'compiled_runtime_projection_status',
    'v3_contracts::SOURCE_HASH',
    'contractAbi',
    'runtimeConfig',
  ],
};

const REQUIRED_SOURCE_FALLBACK = {
  file: 'crates/missiond-daemon/src/context/v3_blueprint_runtime/source_fallback.rs',
  tokens: [
    'ALLOW_SOURCE_FALLBACK_ENV',
    'MISSIOND_V3_ALLOW_SOURCE_FALLBACK',
    'COMPILE_RUNTIME_ACTION',
    'node scripts/compile-v3-runtime.mjs --json',
    'cfg!(debug_assertions) || cfg!(test)',
    'return false;',
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
    'runOcamlProjectDirChecks',
    "'check-project-dir'",
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
        'validate_project_dir',
        'active_project_lisp_file',
        'validate_project_function_form',
        'project.core_step_logic',
        'validate_auth_domain_structured_dir',
        'validate_m6_depth_dir',
        'validate_domain_hardening_dir',
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
  'check-workstation-config',
  'check-workflow-dir',
  'check-genome-dir',
  'emit-genomes',
  'emit-runtime-config',
  'emit-contract-abi',
  'emit-plan-contract',
  'check-plan-contract',
  'compiled-runtime-config.json',
  'compiled-contract-abi.json',
  'project-v3-contracts.mjs',
  'generated V3 contract ABI',
  'genome-runtime',
  'check-m6-depth',
  'check-domain-hardening-deprecated-alias',
  'tools/missiond_lispc/bin/schema_v3.ml',
  'tools/missiond_lispc/bin/workstation_schema.ml',
  'tools/missiond_lispc/bin/emit_json.ml',
  'scripts/lib/v3_compiled_contract.mjs',
  'node scripts/check-typed-lisp-compiler.mjs',
  '(compiler-plane',
  ':semantic-compiler missiond-lispc',
  ':runtime-abi ".missiond/v3/runtime/compiled/*.json"',
  ':payload-fields [:source_units',
  'Freshness is source_hash plus source_units',
  'rust-scan_keyword_pairs',
];

const REQUIRED_WORKFLOW_PROJECTIONS = [
  'bus-refactor',
  'commit-lisp-convergence',
  'conversation-memory-distillation',
  'nightly-evolution',
  'pillar-refactor',
  'project-m6-depth',
  'project-ssot-convergence',
  'multi-project-m6-wave',
  'typed-lisp-compiler-cleanup',
  'typed-lisp-compiler-convergence',
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

  const sourceFallback = read(REQUIRED_SOURCE_FALLBACK.file);
  for (const token of REQUIRED_SOURCE_FALLBACK.tokens) {
    if (!sourceFallback.includes(token)) {
      diagnostics.push(diag(REQUIRED_SOURCE_FALLBACK.file, 'SOURCE_FALLBACK_TOKEN_MISSING', `missing token ${JSON.stringify(token)}`));
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
    const compiledContract = loadCompiledV3Contract({ blueprint: BLUEPRINT, semanticIr: true });
    const expectedSurfaces = compiledSurfaceIds(compiledContract);
    if (!compiledContract.ok || expectedSurfaces.length === 0) {
      for (const d of compiledContract.diagnostics) {
        diagnostics.push({
          ...d,
          code: d.code ?? 'COMPILED_V3_CONTRACT_FAILED',
        });
      }
      if (expectedSurfaces.length === 0) {
        diagnostics.push(diag(
          'scripts/lib/v3_compiled_contract.mjs',
          'COMPILED_V3_SURFACES_MISSING',
          'missiond-lispc emit-v3/emit-semantic-ir must project implementation surfaces for downstream checkers',
        ));
      }
    }
    ocaml = runLispc([
      'check-v3',
      '--blueprint',
      BLUEPRINT,
      '--expected-surfaces',
      (expectedSurfaces.length > 0 ? expectedSurfaces : EXPECTED_SURFACES).join(','),
    ]);
    if (!ocaml.ok) {
      for (const d of ocaml.diagnostics ?? []) diagnostics.push({ ...d, code: d.code ?? 'OCAML_CHECK_FAILED' });
    }
    const planContractSmoke = '/tmp/missiond-plan-contract-smoke.lisp';
    fs.writeFileSync(
      planContractSmoke,
      '(plan :target "mission_execution" :objective "ship" (node :id "n1" :target "mission_execution"))\n',
    );
    for (const argv of [
      ['emit-v3', '--blueprint', BLUEPRINT],
      ['emit-runtime-config', '--blueprint', BLUEPRINT],
      ['emit-semantic-ir', '--blueprint', BLUEPRINT],
      ['emit-contract-abi', '--blueprint', BLUEPRINT],
      ['emit-universe', '--blueprint', BLUEPRINT],
      ['emit-workflows', '--workflow-dir', '.missiond/workflows'],
      ['emit-genomes', '--genome-dir', '.missiond/v3/genome'],
      ['emit-plan-contract', '--file', planContractSmoke],
      ['check-workstation-config', '--blueprint', BLUEPRINT],
      ['check-plan-contract', '--file', planContractSmoke],
      ['check-workflow-dir', '--workflow-dir', '.missiond/workflows'],
      ['check-genome-dir', '--genome-dir', '.missiond/v3/genome'],
      ['check-project-dir', '--dir', '.missiond/frontend'],
    ]) {
      const emit = runLispc(argv);
      emitChecks.push({ argv, ok: emit.ok === true, diagnostics: emit.diagnostics ?? [] });
      if (!emit.ok) {
        diagnostics.push(diag('tools/missiond_lispc', 'OCAML_EMIT_FAILED', `emit command failed: ${argv.join(' ')}`));
      } else if (argv[0].startsWith('emit') && !emit.compiled) {
        diagnostics.push(diag('tools/missiond_lispc', 'OCAML_EMIT_FAILED', `emit command did not return compiled payload: ${argv.join(' ')}`));
      } else if (argv[0] === 'emit-v3') {
        const functions = emit.compiled?.payload?.functions ?? [];
        const surfaces = emit.compiled?.payload?.surfaces ?? [];
        if (!Array.isArray(functions) || functions.length === 0) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_V3_PROJECTION_MISSING_FUNCTIONS', 'emit-v3 must project structured functions[]'));
        }
        if (!Array.isArray(surfaces) || surfaces.length === 0) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_V3_PROJECTION_MISSING_SURFACES', 'emit-v3 must project structured surfaces[]'));
        }
        const typedCompiler = functions.find((fn) => fn.id === 'typed-lisp-compiler');
        if (!typedCompiler || typedCompiler.surface !== 'typed-lisp-compiler') {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_V3_PROJECTION_MISSING_TYPED_COMPILER', 'emit-v3 must project the typed-lisp-compiler function with its surface'));
        }
        if (!Array.isArray(typedCompiler?.steps) || !typedCompiler.steps.includes('s10')) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_V3_PROJECTION_MISSING_FUNCTION_STEPS', 'emit-v3 must project V3 function step ids'));
        }
      } else if (argv[0] === 'emit-runtime-config') {
        const payload = emit.compiled?.payload ?? {};
        if (!Array.isArray(payload.source_units) || payload.source_units.length === 0) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_RUNTIME_CONFIG_SOURCE_UNITS_MISSING', 'emit-runtime-config must project payload.source_units for source_hash freshness'));
        }
        const requiredGroups = [
          'workstation',
          'flow',
          'compute',
          'minimax',
          'router',
          'cascade',
          'projectRegistry',
          'capabilityGovernance',
          'memoryKb',
          'conversationIngestion',
          'autopilot',
          'learningEngine',
        ];
        for (const group of requiredGroups) {
          if (!payload[group] || typeof payload[group] !== 'object') {
            diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_RUNTIME_CONFIG_GROUP_MISSING', `emit-runtime-config must project payload.${group}`));
          }
        }
        if (emit.compiled?.schema_version !== 'missiond.compiled-runtime-config.v1') {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_RUNTIME_CONFIG_SCHEMA_MISMATCH', 'emit-runtime-config must use schema missiond.compiled-runtime-config.v1'));
        }
        if (!payload.workstation?.timeout_policy || !payload.router?.default_chat_model || !payload.autopilot?.boardtask_timeout_policy) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_RUNTIME_CONFIG_POLICY_MISSING', 'emit-runtime-config must project workstation/router/autopilot runtime policy fields'));
        }
      } else if (argv[0] === 'emit-semantic-ir') {
        const facts = emit.compiled?.payload?.facts ?? [];
        const factKinds = new Set(facts.map((fact) => fact.kind));
        for (const kind of ['artifact_contract', 'surface', 'function', 'runtime_policy', 'checker_registry', 'module_source_unit']) {
          if (!factKinds.has(kind)) {
            diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_SEMANTIC_IR_FACT_KIND_MISSING', `emit-semantic-ir must project ${kind} facts`));
          }
        }
        const runtimePolicies = facts.filter((fact) => fact.kind === 'runtime_policy');
        if (runtimePolicies.length < 5) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_SEMANTIC_IR_RUNTIME_POLICIES_INCOMPLETE', 'emit-semantic-ir must project runtime policy facts for compiled ABI consumers'));
        }
      } else if (argv[0] === 'emit-contract-abi') {
        const payload = emit.compiled?.payload ?? {};
        const facts = payload.facts ?? [];
        const factKinds = new Set(facts.map((fact) => fact.kind));
        if (emit.compiled?.schema_version !== 'missiond.contract-abi.v1') {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_CONTRACT_ABI_SCHEMA_MISMATCH', 'emit-contract-abi must use schema missiond.contract-abi.v1'));
        }
        if (!Array.isArray(payload.surfaces) || payload.surfaces.length === 0 || !Array.isArray(payload.functions) || payload.functions.length === 0) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_CONTRACT_ABI_SURFACE_FUNCTIONS_MISSING', 'emit-contract-abi must project surfaces[] and functions[]'));
        }
        for (const kind of ['artifact_contract', 'runtime_policy', 'checker_registry', 'module_source_unit']) {
          if (!factKinds.has(kind)) {
            diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_CONTRACT_ABI_FACT_KIND_MISSING', `emit-contract-abi must project ${kind} facts`));
          }
        }
        if (payload.plan_contract?.schema_version !== 'missiond.plan-contract.v1') {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_CONTRACT_ABI_PLAN_SCHEMA_MISSING', 'emit-contract-abi must include plan_contract schema metadata'));
        }
      } else if (argv[0] === 'emit-plan-contract') {
        const payload = emit.compiled?.payload ?? {};
        if (emit.compiled?.schema_version !== 'missiond.plan-contract.v1') {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_PLAN_CONTRACT_SCHEMA_MISMATCH', 'emit-plan-contract must use schema missiond.plan-contract.v1'));
        }
        if (payload.head !== 'plan' || payload.hints?.target !== 'mission_execution' || !Array.isArray(payload.nodes) || payload.nodes.length !== 1) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_PLAN_CONTRACT_PROJECTION_MISSING', 'emit-plan-contract must project plan head, top-level hints, and nodes[]'));
        }
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
        const workflows = emit.compiled?.payload?.workflows ?? [];
        const names = new Set(workflows.map((workflow) => workflow.name));
        for (const required of REQUIRED_WORKFLOW_PROJECTIONS) {
          if (!names.has(required)) {
            diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_WORKFLOW_PROJECTION_MISSING_REQUIRED_WORKFLOW', `emit-workflows must project workflow ${required}`));
          }
        }
        for (const workflow of workflows) {
          if (!workflow.workflow_id || !workflow.status) {
            diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_WORKFLOW_PROJECTION_MISSING_ID_STATUS', `workflow projection ${workflow.name ?? '<unknown>'} must include workflow_id and status`));
          }
          if (!Array.isArray(workflow.steps) || workflow.steps.length === 0) {
            diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_WORKFLOW_PROJECTION_MISSING_STEPS', `workflow projection ${workflow.name ?? '<unknown>'} must include steps`));
          }
          if (!Number.isInteger(workflow.risk_gate_count) || workflow.risk_gate_count <= 0) {
            diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_WORKFLOW_PROJECTION_MISSING_RISK_GATES', `workflow projection ${workflow.name ?? '<unknown>'} must include risk gates`));
          }
          if (!Number.isInteger(workflow.completion_criteria_count) || workflow.completion_criteria_count <= 0) {
            diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_WORKFLOW_PROJECTION_MISSING_COMPLETION', `workflow projection ${workflow.name ?? '<unknown>'} must include completion criteria`));
          }
        }
        const m6Depth = emit.compiled?.payload?.workflows?.find((workflow) => workflow.name === 'project-m6-depth');
        if (!Array.isArray(m6Depth?.steps) || !m6Depth.steps.includes('s10')) {
          diagnostics.push(diag('tools/missiond_lispc/bin/emit_json.ml', 'OCAML_WORKFLOW_PROJECTION_MISSING_STRUCTURED_STEPS', 'emit-workflows must project step ids from structured (step sN ...) forms'));
        }
      } else if (argv[0] === 'emit-genomes') {
        const genomes = emit.compiled?.payload?.genomes ?? [];
        if (!Array.isArray(genomes) || genomes.length === 0) {
          diagnostics.push(diag('tools/missiond_lispc/bin/genome_schema.ml', 'OCAML_GENOME_PROJECTION_MISSING_GENOMES', 'emit-genomes must project structured genomes[]'));
        }
        const autopilot = genomes.find((genome) => genome.id === 'missiond-autopilot');
        if (!autopilot) {
          diagnostics.push(diag('tools/missiond_lispc/bin/genome_schema.ml', 'OCAML_GENOME_PROJECTION_MISSING_AUTOPILOT', 'emit-genomes must project missiond-autopilot genome'));
        }
      }
    }
    if (fs.existsSync(DEFAULT_AUTH_MISSIOND)) {
      const authM6Depth = runLispc(['check-m6-depth', '--dir', DEFAULT_AUTH_MISSIOND]);
      emitChecks.push({
        argv: ['check-m6-depth', '--dir', DEFAULT_AUTH_MISSIOND],
        ok: authM6Depth.ok === true,
        diagnostics: authM6Depth.diagnostics ?? [],
      });
      if (!authM6Depth.ok) {
        diagnostics.push(diag('tools/missiond_lispc/bin/project_schema.ml', 'OCAML_AUTH_M6_DEPTH_GATE_FAILED', 'Auth M6 depth structural sample gate failed'));
        for (const d of authM6Depth.diagnostics ?? []) diagnostics.push({ ...d, code: d.code ?? 'OCAML_AUTH_M6_DEPTH_DIAGNOSTIC' });
      }
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
    if (file === BLUEPRINT) return readBlueprintWithEvidenceSidecars(process.cwd(), file);
    return fs.readFileSync(file, 'utf8');
  } catch {
    return '';
  }
}

function diag(file, code, message) {
  return { file, line: 1, column: 1, code, message, path: '' };
}

main();
