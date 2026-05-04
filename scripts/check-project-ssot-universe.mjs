#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-project-ssot-universe.mjs [--json]

Checks MissionD multi-project SSOT registry convergence:
  - V3 project-blueprint-registry names MissionD, Forge, Part1 devtools (jarvis,
    jarvis-mechanic, xjpcode, neural-codegen, semantic-terminal), XJP services,
    and PCEA.
  - V3 service-runtime-universe exposes production service deployment facts.
  - project-ssot-convergence workflow exists.
  - XJP and PCEA local SSOT checkers pass; Part1 devtools roots exist and their
    cheap/static project-local runners (where present) exit 0.
`;

const PROJECTS = [
  // Part1 devtools — sibling repos with project-local SSOT (root-existence + cheap/static checker invocation).
  { id: 'jarvis', root: '/Users/jinchen/Projects/jarvis' },
  { id: 'jarvis-forge', root: '/Users/jinchen/Projects/jarvis-forge' },
  { id: 'jarvis-mechanic', root: '/Users/jinchen/Projects/jarvis-mechanic', checker: ['node', ['scripts/check-mechanic-ssot.mjs']] },
  { id: 'xjpcode', root: '/Users/jinchen/Projects/xjpcode', checker: ['node', ['scripts/check-xjpcode-ssot-complete.mjs', '--json']] },
  { id: 'neural-codegen', root: '/Users/jinchen/Projects/neural-codegen', checker: ['bash', ['.missiond/check.sh', '--dry-run']] },
  { id: 'semantic-terminal', root: '/Users/jinchen/Projects/semantic-terminal', checker: ['bash', ['.missiond/check.sh', '--dry-run']] },
  // XJP services + PCEA.
  { id: 'xiaojinpro-backend', root: '/Users/jinchen/Projects/xiaojinpro-backend', checker: ['node', ['scripts/check-xjp-ssot-complete.mjs', '--json']] },
  { id: 'deploy-center', root: '/Users/jinchen/Projects/xiaojinpro-backend/services/deploy-center' },
  { id: 'deploy-agent', root: '/Users/jinchen/Projects/xiaojinpro-backend/crates/xjp-cli' },
  { id: 'auth', root: '/Users/jinchen/Projects/xiaojinpro-backend/services/auth' },
  { id: 'router', root: '/Users/jinchen/Projects/xiaojinpro-backend/services/router' },
  { id: 'payments', root: '/Users/jinchen/Projects/xiaojinpro-backend/services/payments' },
  { id: 'asr', root: '/Users/jinchen/Projects/xiaojinpro-backend/services/asr' },
  { id: 'timeline', root: '/Users/jinchen/Projects/xiaojinpro-backend/services/timeline' },
  { id: 'pcea', root: '/Users/jinchen/Downloads/PCEA develop', checker: ['node', ['scripts/check-pcea-ssot-complete.mjs', '--json']] },
];

function main() {
  const args = process.argv.slice(2);
  const json = args.includes('--json');
  if (args.some((a) => !['--json', '--help', '-h'].includes(a))) {
    console.error(usage);
    process.exit(2);
  }
  if (args.includes('--help') || args.includes('-h')) {
    console.log(usage);
    process.exit(0);
  }

  const diagnostics = [];
  const blueprint = readBlueprintWithEvidenceSidecars(process.cwd(), '.missiond/v3/missiond-blueprint.lisp');
  requireAll(diagnostics, '.missiond/v3/missiond-blueprint.lisp', blueprint, [
    '(project-blueprint-registry',
    ':id jarvis-forge',
    ':id jarvis',
    ':id jarvis-mechanic',
    ':id xjpcode',
    ':id neural-codegen',
    ':id semantic-terminal',
    ':id xiaojinpro-backend',
    ':id deploy-center',
    ':id deploy-agent',
    ':id auth',
    ':id router',
    ':id payments',
    ':id asr',
    ':id timeline',
    ':id pcea',
    '/Users/jinchen/Downloads/PCEA develop',
    '(service-runtime-universe',
    ':schema "missiond.service-runtime-universe.v1"',
    '(service :id auth',
    ':project xiaojinpro-backend',
    ':root "/Users/jinchen/Projects/xiaojinpro-backend/services/auth"',
    ':public-base-url "https://auth.xiaojinpro.com"',
    ':issuer "https://auth.xiaojinpro.com"',
    ':domains ["auth.xiaojinpro.com"]',
    ':dns-provider cloudflare',
    ':mutate requires-board-approval',
    ':deployment (:substrate kubernetes :namespace production :deployment "xjp-auth-center" :service "xjp-auth-center"',
    ':proxy (:kind caddy :domain "auth.xiaojinpro.com"',
    ':health ["/health/live" "/health/ready" "/.well-known/openid-configuration" "/.well-known/jwks.json"]',
    ':event-ingest (:endpoint "/webhooks/auth-event" :domain system :event ExternalServiceEvent',
    ':source auth-audit-events',
    ':authority provider-durable-log-first',
    ':dependencies [postgres redis secret-store wechat-open-platform google-oauth sms-provider email-provider]',
    ':ops-capability deploy-ops',
    '(capability :id cloudflare-dns',
    ':default-mode read-only-inventory',
    'explicit Board approval',
  ]);

  requireExistingText(diagnostics, '/Users/jinchen/Projects/xiaojinpro-backend/services/auth/.missiond/intent.lisp', [
    ':public-base-url "https://auth.xiaojinpro.com"',
    ':issuer "https://auth.xiaojinpro.com"',
    ':status migration-complete-runtime',
    ':runtime-database postgresql',
    ':status superseded-by-lisp',
    ':status token-storage-consumer-boundary',
    ':risk wechat-callback-prod-drift',
  ]);
  requireExistingText(diagnostics, '/Users/jinchen/Projects/xiaojinpro-backend/services/auth/.missiond/backend/auth-backend-blueprint.lisp', [
    'production-runtime-deployment',
    'kb-decision-boundary',
    'k8s/production/deployment.yaml',
    'caddy/Caddyfile',
  ]);

  const workflowPath = '.missiond/workflows/project-ssot-convergence.lisp';
  if (!fs.existsSync(workflowPath)) {
    diagnostics.push({ file: workflowPath, message: 'missing project SSOT convergence workflow' });
  } else {
    const workflow = fs.readFileSync(workflowPath, 'utf8');
    requireAll(diagnostics, workflowPath, workflow, [
      '(workflow project-ssot-convergence',
      ':schema "missiond.workflow.project-ssot-convergence.v1"',
      'collect-evidence',
      'draft-l1-index',
      'draft-backend-blueprint',
      'draft-frontend-blueprint',
      'create-checkers',
      'dispatch-backfill-workers',
      'verify-and-report',
    ]);
  }

  const checkerResults = [];
  for (const project of PROJECTS) {
    if (!fs.existsSync(project.root)) {
      diagnostics.push({ file: project.root, message: `missing project root for ${project.id}` });
      continue;
    }
    if (project.checker) {
      const [cmd, args] = project.checker;
      const proc = spawnSync(cmd, args, { cwd: project.root, encoding: 'utf8', timeout: 60_000 });
      const ok = proc.status === 0 && !proc.error;
      checkerResults.push({
        id: project.id,
        ok,
        command: `${cmd} ${args.join(' ')}`,
        stdout_tail: tail(proc.stdout ?? ''),
        stderr_tail: tail(proc.stderr ?? ''),
        error: proc.error?.message ?? null,
      });
      if (!ok) diagnostics.push({ file: project.root, message: `project checker failed: ${cmd} ${args.join(' ')}` });
    }
  }

  const result = { ok: diagnostics.length === 0, projects: PROJECTS.map((p) => p.id), checkerResults, diagnostics };
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(`project SSOT universe check OK (${PROJECTS.length} projects/services)`);
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`project SSOT universe check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push({ file, message: `missing required text: ${needle}` });
  }
}

function requireExistingText(diagnostics, file, needles) {
  if (!fs.existsSync(file)) {
    diagnostics.push({ file, message: 'missing required path' });
    return;
  }
  const source = fs.readFileSync(file, 'utf8');
  requireAll(diagnostics, file, source, needles);
}

function tail(text, lines = 6) {
  return text ? text.split('\n').slice(-lines).join('\n').trim() : '';
}

main();
