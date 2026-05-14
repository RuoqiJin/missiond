#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-data-residency-universe-isomorphism.mjs [--json]

Checks MissionD's data-residency universe contract:
  - V3 declares cn/global hard partitions, global-eu operating-zone policy,
    and XJP platform partitions that apps bind to.
  - PCEA carries a project-local data-residency declaration.
  - PCEA local checker pins the new data-bearing M6 requirement.
`;

const REPO = process.cwd();
const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  report: '.missiond/research/data-residency-universe-report-20260512.md',
  pceaIntent: '/Users/jinchen/Downloads/PCEA develop/.missiond/intent.lisp',
  pceaCheck: '/Users/jinchen/Downloads/PCEA develop/.missiond/check.sh',
};

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const diagnostics = [];

  const blueprint = readText(diagnostics, FILES.blueprint, true);
  const report = readText(diagnostics, FILES.report, false);
  const pceaIntent = readText(diagnostics, FILES.pceaIntent, false);
  const pceaCheck = readText(diagnostics, FILES.pceaCheck, false);

  requireAll(diagnostics, FILES.blueprint, blueprint, [
    '(data-residency-universe',
    ':schema "missiond.data-residency-universe.v1"',
    '(data-region-partition-contract',
    '(xjp-platform-partition-contract',
    'xjp-cn',
    'xjp-global',
    'xjp-global-eu',
    ':application-bindings [pcea-cn cuthub-cn]',
    ':application-bindings [pcea-global cuthub-global]',
    ':application-bindings [pcea-global-eu]',
    '(regional-auth-issuer-contract',
    '(regional-secret-store-contract',
    '(regional-runtime-target-contract',
    '(regional-storage-contract',
    '(regional-payment-ledger-contract',
    '(regional-router-model-policy',
    '(cross-region-data-policy',
    ':default deny',
    '(project-region-declaration :project pcea',
    ':data-regions [cn global]',
    ':primary-region global',
    ':operating-zones [global-us global-eu]',
    ':contains-personal-data true',
    ':contains-spi true',
    ':contains-important-data unknown',
    ':cross-region-default deny',
    'anonymized-aggregate-metrics',
    'public-content',
    'security-threat-intelligence',
    'compliance-approved-export',
    'code-and-artifacts',
    'https://auth.pcea.cn',
    'https://auth.pcea.io',
    ':runtime-target ecs-pcea',
    ':runtime-provider aliyun-ecs',
    ':platform xjp-cn',
    ':runtime-target gcp-runtime',
    ':runtime-provider gcp-vm',
    ':platform xjp-global',
    ':platform xjp-global-eu',
    'google-cloud-storage',
    'gs://xjp-global-object-store-project-20250408',
    'gcs-global-asia',
    'gcs-global-eu-pending',
    'target-pending-provisioning',
    'must-not-build-on-target',
    'cuthub.cn',
    'cuthub.com',
    'cuthub-cn :platform xjp-cn',
    'cuthub-global :platform xjp-global',
    'parent-domain-cookie-sharing',
    '(surface data-residency-universe',
    'scripts/check-v3-data-residency-universe-isomorphism.mjs',
  ]);

  requireAll(diagnostics, FILES.report, report, [
    'MissionD should model data residency as a first-class',
    'It is an XJP platform split',
    'PCEA-CN and PCEA-GLOBAL must use separate issuers',
    'Cross-region data movement is default-deny',
    'https://www.cac.gov.cn/2024-03/22/c_1712776611775634.htm',
    'https://docs.aws.amazon.com/whitepapers/latest/aws-fault-isolation-boundaries/partitions.html',
  ]);

  requireAll(diagnostics, FILES.pceaIntent, pceaIntent, [
    ':data-residency',
    '(schema "pcea.data-residency.v2")',
    '(data-regions [cn global])',
    '(primary-region global)',
    '(operating-zones [global-us global-eu])',
    '(contains-personal-data true)',
    '(contains-spi true)',
    '(contains-important-data unknown)',
    '(cross-region-default deny)',
    'anonymized-aggregate-metrics',
    'public-content',
    'security-threat-intelligence',
    'compliance-approved-export',
    'code-and-artifacts',
    '(platform-partition-binding',
    ':platform xjp-cn',
    ':platform xjp-global',
    ':platform xjp-global-eu',
    '(partition pcea-cn',
    ':issuer "https://auth.pcea.cn"',
    ':audience pcea-cn',
    ':runtime-target ecs-pcea',
    ':runtime-provider aliyun-ecs',
    '(partition pcea-global',
    ':issuer "https://auth.pcea.io"',
    ':audience pcea-global',
    ':runtime-target gcp-runtime',
    ':runtime-provider gcp-vm',
    'target-pending-provisioning',
    '(operating-zone global-eu',
    ':audience pcea-global-eu',
    'single-instance-multi-region',
    'ip-only-region-binding',
  ]);

  requireAll(diagnostics, FILES.pceaCheck, pceaCheck, [
    'intent :data-residency',
    'intent data residency schema v2',
    'intent platform binding contract',
    'intent pcea-cn platform xjp-cn',
    'intent pcea-global platform value',
    'intent pcea-global-eu platform value',
    'intent data regions cn/global',
    'intent contains SPI',
    'intent important data unknown',
    'intent cross-region default deny',
    'intent pcea-cn issuer',
    'intent pcea-global issuer',
    'intent CN runtime target ECS',
    'intent CN runtime provider Aliyun ECS',
    'intent global runtime target GCP',
    'intent global runtime provider GCP VM',
    'intent deploy-center authority',
    'intent no build on ECS target',
    'intent EU operating zone',
    'intent anonymous aggregate egress',
    'intent code artifacts egress',
  ]);

  const ok = diagnostics.length === 0;
  const result = { ok, diagnostics };
  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (ok) {
    console.log('v3 data-residency universe check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 data-residency universe check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(ok ? 0 : 1);
}

function parseArgs(args) {
  const opts = { json: false };
  for (const arg of args) {
    if (arg === '--json') opts.json = true;
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

function readText(diagnostics, file, blueprint) {
  try {
    if (blueprint) return readBlueprintWithEvidenceSidecars(REPO, file);
    const abs = path.isAbsolute(file) ? file : path.join(REPO, file);
    return fs.readFileSync(abs, 'utf8');
  } catch (err) {
    diagnostics.push({ file, message: `cannot read: ${err.message}` });
    return '';
  }
}

function requireAll(diagnostics, file, text, needles) {
  for (const needle of needles) {
    if (!text.includes(needle)) diagnostics.push({ file, message: `missing ${needle}` });
  }
}

main();
