#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const DEFAULT_PROJECT_ROOT = '/Users/jinchen/Projects';

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    const raw = argv[i];
    if (!raw.startsWith('--')) continue;
    const eq = raw.indexOf('=');
    if (eq > -1) {
      args[raw.slice(2, eq)] = raw.slice(eq + 1);
      continue;
    }
    const key = raw.slice(2);
    const next = argv[i + 1];
    if (!next || next.startsWith('--')) {
      args[key] = true;
    } else {
      args[key] = next;
      i += 1;
    }
  }
  return args;
}

function requiredArg(args, key) {
  const value = args[key];
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`missing required --${key}`);
  }
  return value.trim();
}

function kebab(value) {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

function snake(value) {
  return kebab(value).replace(/-/g, '_');
}

function upperSnake(value) {
  return snake(value).toUpperCase();
}

function titleFromId(value) {
  return kebab(value)
    .split('-')
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function quote(value) {
  return JSON.stringify(value);
}

function writeFile(root, rel, content, dryRun) {
  const target = path.join(root, rel);
  if (!dryRun) {
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, content.endsWith('\n') ? content : `${content}\n`);
    if (rel.endsWith('.sh')) fs.chmodSync(target, 0o755);
  }
  return rel;
}

function buildConfig(args) {
  const projectId = kebab(requiredArg(args, 'project-id'));
  if (!/^[a-z0-9][a-z0-9-]*[a-z0-9]$/.test(projectId)) {
    throw new Error(`invalid project id: ${projectId}`);
  }
  const projectSnake = snake(projectId);
  const projectUpper = upperSnake(projectId);
  const root = path.resolve(
    String(args.out || args.root || path.join(DEFAULT_PROJECT_ROOT, projectId)),
  );
  const name = String(args.name || titleFromId(projectId)).trim();
  const frontendDomain = String(args.domain || `${projectId}.xiaojinpro.top`).trim();
  const apiDomain = String(args['api-domain'] || `${projectId}-api.xiaojinpro.top`).trim();
  const apiBaseUrl = `https://${apiDomain}`;
  const frontendUrl = `https://${frontendDomain}`;
  const backendService = String(args['backend-service'] || `${projectId}-backend`).trim();
  const backendPort = Number(args['backend-port'] || 8080);
  if (!Number.isInteger(backendPort) || backendPort <= 0 || backendPort > 65535) {
    throw new Error(`invalid --backend-port: ${args['backend-port']}`);
  }
  const image = String(args.image || `ghcr.io/ruoqijin/${backendService}`).trim();
  const imageEnv = `${upperSnake(backendService)}_IMAGE`;
  const database = String(args.database || projectSnake).trim();
  const vercelProject = String(args['vercel-project'] || projectId).trim();
  const runtimeTarget = String(
    args['runtime-target'] || `gcp-runtime:xjp-backend:${backendService}`,
  ).trim();
  const builder = String(args.builder || 'privatecloud-10900kf').trim();
  const dockerNetwork = String(args['docker-network'] || 'xiaojinpro_default').trim();
  const supportAddress = String(args['support-address'] || `support@${frontendDomain}`).trim();
  return {
    projectId,
    projectSnake,
    projectUpper,
    root,
    name,
    frontendDomain,
    apiDomain,
    apiBaseUrl,
    frontendUrl,
    backendService,
    backendPort,
    image,
    imageEnv,
    database,
    vercelProject,
    runtimeTarget,
    builder,
    dockerNetwork,
    supportAddress,
    secretPrefix: `projects/${projectId}/prod`,
  };
}

function manifestToml(c) {
  return `schema_version = "1"

[service]
name = ${quote(c.backendService)}
deploy_project = ${quote(c.projectId)}
language = "rust"

[healthcheck]
shallow = "/api/health"
deep = "/api/health/deep"

[deploy]
smoke_base_url = ${quote(c.apiBaseUrl)}

[env.required.DATABASE_URL]
doc = "Secret Store ref: ${c.secretPrefix}/DATABASE_URL."

[env.required.XJP_AUTH_ISSUER]
doc = "Secret Store ref: ${c.secretPrefix}/XJP_AUTH_ISSUER."

[env.required.XJP_AUTH_JWKS_URL]
doc = "Secret Store ref: ${c.secretPrefix}/XJP_AUTH_JWKS_URL."

[env.required.XJP_AUTH_AUDIENCE]
doc = "Secret Store ref: ${c.secretPrefix}/XJP_AUTH_AUDIENCE."

[env.required.SERVICE_API_TOKEN]
doc = "Secret Store ref: ${c.secretPrefix}/SERVICE_API_TOKEN for machine endpoints."

[env.optional.PORT]
default = ${quote(String(c.backendPort))}
doc = "HTTP listen port."

[env.optional.RUST_LOG]
default = ${quote(`${c.projectSnake}_backend=info,tower_http=info`)}
doc = "Rust tracing filter."

[[smoke]]
name = "api-health"
method = "GET"
path = "/api/health"
expect_status = 200

[[smoke]]
name = "deep-readiness"
method = "GET"
path = "/api/health/deep"
expect_status = 200

[deps.postgres]
kind = "postgres"

[deps.auth]
kind = "http"
url_env = "XJP_AUTH_ISSUER"
`;
}

function deployCenterProject(c) {
  return {
    schema: 'missiond.deploy-center-project-template.v1',
    project: c.projectId,
    service: c.backendService,
    change_class: 'product_service_release',
    deploy_center_slug: c.projectId,
    deployment_policy: {
      manifest_required: true,
      immutable_image_required: true,
      runtime_digest_required: true,
      smoke_required: true,
      db_adoption_required: true,
      release_lease_required: true,
      artifact_lane: 'privatecloud-registry-image',
      target_side_build_allowed: false,
      approval_policy: 'prod-policy-or-explicit-board-approval',
      diagnostic_profiles: [
        'deploy_provenance_snapshot',
        'container_inventory',
        'dependency_manifest_scan',
        'supply_chain_ioc_scan',
      ],
    },
    build_lane: {
      authority: 'deploy-center',
      builder: c.builder,
      source_sync: 'deploy-center/codebase source sync',
      dockerfile: 'backend/Dockerfile',
      image: c.image,
      required_digest: `${c.image}@sha256:<digest>`,
    },
    runtime_target: {
      authority: 'deploy-center',
      id: c.runtimeTarget,
      kind: 'gcp-vm-compose',
      compose_files: ['deploy/gcp-vm/compose.yaml'],
      image_env: c.imageEnv,
      docker_network: c.dockerNetwork,
      target_side_build_allowed: false,
    },
    release_closure: {
      state_machine: [
        'classify_change',
        'preflight',
        'build_candidate',
        'acquire_release_lease',
        'deploy',
        'runtime_observe',
        'deep_smoke',
        'closure_verdict',
        'release_or_rollback',
      ],
      success_authority: 'ReleaseEvidence + ClosureVerdict',
      fail_closed_blockers: [
        'service_manifest_missing',
        'deploy_center_slug_missing',
        'runtime_target_missing',
        'secret_store_ref_missing',
        'db_adoption_plan_missing',
        'runtime_digest_mismatch',
        'release_lease_conflict',
      ],
    },
    smoke: [
      { name: 'api-health', url: `${c.apiBaseUrl}/api/health`, expect_status: 200 },
      { name: 'deep-readiness', url: `${c.apiBaseUrl}/api/health/deep`, expect_status: 200 },
      { name: 'frontend-home', url: c.frontendUrl, expect_status: 200 },
    ],
    rollback: {
      image_ref: `${c.image}@sha256:<previous_digest>`,
      compose_files: ['deploy/gcp-vm/compose.yaml'],
      database_policy: 'manual rollback/adoption plan required for irreversible migrations',
    },
  };
}

function closurePreflight(c) {
  return {
    schema: 'missiond.product-deployment-closure-preflight.v1',
    project: c.projectId,
    service: c.backendService,
    deployment_intent: {
      project: c.projectId,
      service: c.backendService,
      target: 'prod',
      change_class: 'product_service_release',
      desired_commit: '<git-sha>',
      deployment_policy_hash: '<compiled-policy-hash>',
    },
    required_assets: [
      'service.manifest.toml',
      'deploy/deploy-center/project.json',
      'deploy/deployment-closure/runtime-target.json',
      'deploy/deployment-closure/db-adoption-plan.json',
      'deploy/deployment-closure/domain-plan.json',
      'deploy/deployment-closure/rollback-plan.json',
      'vercel.json',
      '.missiond/operations/<project-id>-operations-blueprint.lisp',
    ],
    release_candidate_requirements: {
      git_sha: true,
      image_digest: true,
      builder: c.builder,
      artifact_lane: 'privatecloud-registry-image',
      manifest_hash: true,
      compiled_abi_hash: true,
      migration_plan: 'deploy/deployment-closure/db-adoption-plan.json',
      rollback_artifact: 'deploy/deployment-closure/rollback-plan.json',
    },
    release_lease: {
      required: true,
      runtime_target: c.runtimeTarget,
      expected_running_digest: `${c.image}@sha256:<digest>`,
      conflict_policy: 'fail-closed',
    },
    runtime_observation_required: [
      'running_image_digest',
      'container_id',
      'compose_files',
      'entrypoint',
      'port_binding',
      'health_result',
      'secret_ref_names',
      'db_migration_state',
      'domain_binding',
    ],
    fail_closed_if: [
      'manifest_required_missing',
      'deploy_center_slug_missing',
      'runtime_target_missing',
      'secret_store_ref_missing',
      'db_adoption_plan_missing',
      'runtime_digest_mismatch',
      'release_lease_conflict',
      'smoke_missing_or_failed',
      'deploy_blocked_by_secret_store',
    ],
  };
}

function runtimeTarget(c) {
  return {
    schema: 'missiond.runtime-target-template.v1',
    project: c.projectId,
    service: c.backendService,
    runtime_target: c.runtimeTarget,
    host: 'xjp-backend',
    kind: 'gcp-vm-compose',
    bind: `127.0.0.1:${c.backendPort}`,
    public_base_url: c.apiBaseUrl,
    compose_files: ['deploy/gcp-vm/compose.yaml'],
    image_env: c.imageEnv,
    required_running_digest: `${c.image}@sha256:<digest>`,
    target_side_build_allowed: false,
  };
}

function dbPlan(c) {
  return {
    schema: 'missiond.db-adoption-plan.v1',
    project: c.projectId,
    database: c.database,
    authority: 'deploy-center',
    migration_directory: 'backend/migrations',
    state_required_for_closure: 'adopted',
    production_migrations: {
      automatic_on_startup: false,
      operator_action_required: true,
      evidence_required: ['migration_version', 'applied_at', 'database', 'executor'],
    },
  };
}

function domainPlan(c) {
  return {
    schema: 'missiond.domain-plan.v1',
    project: c.projectId,
    authority: 'xjp-domain-service',
    direct_cloudflare_mutation_allowed: false,
    domains: [
      { host: c.frontendDomain, target: 'vercel-frontend', smoke: c.frontendUrl },
      { host: c.apiDomain, target: c.runtimeTarget, smoke: `${c.apiBaseUrl}/api/health` },
    ],
    support_mailbox: {
      address: c.supportAddress,
      status: 'planned',
      authority: 'xjp-mail-service',
    },
  };
}

function rollbackPlan(c) {
  return {
    schema: 'missiond.rollback-plan.v1',
    project: c.projectId,
    service: c.backendService,
    authority: 'deploy-center',
    artifact_refs: {
      previous_image_digest: `${c.image}@sha256:<previous_digest>`,
      compose_files: ['deploy/gcp-vm/compose.yaml'],
      release_evidence: 'deploy-center:/api/deploy/evidence/<release_id>',
    },
    requires_approval: true,
    post_rollback_evidence: ['runtime_observation', 'deep_smoke', 'closure_verdict'],
  };
}

function vercelProject(c) {
  return {
    schema: 'missiond.vercel-project-template.v1',
    project: c.projectId,
    vercel_project: c.vercelProject,
    root_directory: 'frontend',
    production_domain: c.frontendDomain,
    env: {
      NEXT_PUBLIC_APP_URL: c.frontendUrl,
      NEXT_PUBLIC_API_BASE_URL: `${c.apiBaseUrl}/api`,
    },
    authority: 'vercel',
    closure_note: 'Vercel deploy success is frontend evidence only; backend release closure comes from Deploy Center ClosureVerdict.',
  };
}

function composeYaml(c) {
  return `services:
  backend:
    image: \${${c.imageEnv}:?set immutable image digest}
    restart: unless-stopped
    env_file:
      - .env
    ports:
      - "127.0.0.1:${c.backendPort}:${c.backendPort}"
    networks:
      - ${c.dockerNetwork}

networks:
  ${c.dockerNetwork}:
    external: true
`;
}

function envExample(c) {
  return `${c.imageEnv}=${c.image}@sha256:<digest>
DATABASE_URL=secret-store://${c.secretPrefix}/DATABASE_URL
XJP_AUTH_ISSUER=secret-store://${c.secretPrefix}/XJP_AUTH_ISSUER
XJP_AUTH_JWKS_URL=secret-store://${c.secretPrefix}/XJP_AUTH_JWKS_URL
XJP_AUTH_AUDIENCE=secret-store://${c.secretPrefix}/XJP_AUTH_AUDIENCE
SERVICE_API_TOKEN=secret-store://${c.secretPrefix}/SERVICE_API_TOKEN
PORT=${c.backendPort}
RUST_LOG=${c.projectSnake}_backend=info,tower_http=info
`;
}

function rootVercelJson() {
  return JSON.stringify(
    {
      $schema: 'https://openapi.vercel.sh/vercel.json',
      buildCommand: 'pnpm --dir frontend build',
      installCommand: 'pnpm --dir frontend install',
      outputDirectory: 'frontend/.next',
    },
    null,
    2,
  );
}

function operationsBlueprint(c) {
  return `(${c.projectId}-operations-blueprint
  :schema "missiond.operations-blueprint.v1"
  :project ${c.projectId}
  :status scaffolded-deployment-closure
  :deployment-closure-bundle (:preflight "deploy/deployment-closure/preflight.json"
                               :runtime-target "deploy/deployment-closure/runtime-target.json"
                               :db-adoption-plan "deploy/deployment-closure/db-adoption-plan.json"
                               :domain-plan "deploy/deployment-closure/domain-plan.json"
                               :rollback-plan "deploy/deployment-closure/rollback-plan.json"
                               :success-authority "ReleaseEvidence + ClosureVerdict")
  :vercel (:project ${quote(c.vercelProject)} :domain ${quote(c.frontendDomain)} :root-directory "frontend" :env [NEXT_PUBLIC_APP_URL NEXT_PUBLIC_API_BASE_URL])
  :privatecloud-rust-build-lane (:builder ${c.builder} :authority deploy-center :artifact-lane privatecloud-registry-image :image ${quote(c.image)} :target-side-build-prohibited true)
  :deploy-center (:pipeline-project ${quote(c.projectId)} :project-config "deploy/deploy-center/project.json" :runtime-target ${quote(c.runtimeTarget)} :compose "deploy/gcp-vm/compose.yaml" :backend-domain ${quote(c.apiDomain)} :image-env ${c.imageEnv} :release-lease-required true :closure-verdict-required true)
  :secret-store (:prefix ${quote(`${c.secretPrefix}/`)} :required [DATABASE_URL XJP_AUTH_ISSUER XJP_AUTH_JWKS_URL XJP_AUTH_AUDIENCE SERVICE_API_TOKEN])
  :postgres (:database ${quote(c.database)} :migrations "backend/migrations" :adoption-plan "deploy/deployment-closure/db-adoption-plan.json" :startup-migrations false)
  :domain (:authority xjp-domain-service :plan "deploy/deployment-closure/domain-plan.json" :direct-cloudflare-mutation false)
  :health-smoke [${quote(`${c.apiBaseUrl}/api/health`)} ${quote(`${c.apiBaseUrl}/api/health/deep`)} ${quote(c.frontendUrl)}]
  :rollback (:plan "deploy/deployment-closure/rollback-plan.json" :requires-approval true)
  :acceptance ["service.manifest.toml exists before deploy"
               "deploy-center project slug and runtime target are registered"
               "Secret Store refs exist; values are not written to tracked files"
               "DB adoption evidence exists before ClosureVerdict success"
               "Compose uses immutable image digest and contains no build section"
               "ReleaseLease acquired before runtime mutation"
               "ClosureVerdict success is the only deployed status authority"])`;
}

function checkSh(c) {
  return `#!/usr/bin/env bash
set -euo pipefail

required=(
  "service.manifest.toml"
  "deploy/deploy-center/project.json"
  "deploy/deployment-closure/preflight.json"
  "deploy/deployment-closure/runtime-target.json"
  "deploy/deployment-closure/db-adoption-plan.json"
  "deploy/deployment-closure/domain-plan.json"
  "deploy/deployment-closure/rollback-plan.json"
  "deploy/vercel/project.json"
  "vercel.json"
  ".missiond/operations/${c.projectId}-operations-blueprint.lisp"
)

for file in "\${required[@]}"; do
  test -f "$file" || { echo "missing $file" >&2; exit 1; }
done

grep -q "deploy_project = \\"${c.projectId}\\"" service.manifest.toml
grep -q "deep = \\"/api/health/deep\\"" service.manifest.toml
grep -q "release_lease_required" deploy/deploy-center/project.json
grep -q "target_side_build_allowed.*false" deploy/deploy-center/project.json
grep -q "ClosureVerdict" .missiond/operations/${c.projectId}-operations-blueprint.lisp
if grep -qE '^\\s*build\\s*:' deploy/gcp-vm/compose.yaml; then
  echo "compose must not contain build:" >&2
  exit 1
fi
`;
}

function generate(c, dryRun = false) {
  const files = [];
  files.push(writeFile(c.root, 'service.manifest.toml', manifestToml(c), dryRun));
  files.push(
    writeFile(
      c.root,
      'deploy/deploy-center/project.json',
      JSON.stringify(deployCenterProject(c), null, 2),
      dryRun,
    ),
  );
  files.push(
    writeFile(
      c.root,
      'deploy/deployment-closure/preflight.json',
      JSON.stringify(closurePreflight(c), null, 2),
      dryRun,
    ),
  );
  files.push(
    writeFile(
      c.root,
      'deploy/deployment-closure/runtime-target.json',
      JSON.stringify(runtimeTarget(c), null, 2),
      dryRun,
    ),
  );
  files.push(
    writeFile(
      c.root,
      'deploy/deployment-closure/db-adoption-plan.json',
      JSON.stringify(dbPlan(c), null, 2),
      dryRun,
    ),
  );
  files.push(
    writeFile(
      c.root,
      'deploy/deployment-closure/domain-plan.json',
      JSON.stringify(domainPlan(c), null, 2),
      dryRun,
    ),
  );
  files.push(
    writeFile(
      c.root,
      'deploy/deployment-closure/rollback-plan.json',
      JSON.stringify(rollbackPlan(c), null, 2),
      dryRun,
    ),
  );
  files.push(
    writeFile(c.root, 'deploy/vercel/project.json', JSON.stringify(vercelProject(c), null, 2), dryRun),
  );
  files.push(writeFile(c.root, 'deploy/gcp-vm/compose.yaml', composeYaml(c), dryRun));
  files.push(writeFile(c.root, 'deploy/gcp-vm/.env.example', envExample(c), dryRun));
  files.push(writeFile(c.root, 'vercel.json', rootVercelJson(), dryRun));
  files.push(
    writeFile(
      c.root,
      `.missiond/operations/${c.projectId}-operations-blueprint.lisp`,
      operationsBlueprint(c),
      dryRun,
    ),
  );
  files.push(writeFile(c.root, '.missiond/check.sh', checkSh(c), dryRun));
  return files;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function runSelfTest() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-product-closure-'));
  const c = buildConfig({
    'project-id': 'good-things-daily',
    name: 'Good Things Daily',
    out: root,
    domain: 'goodnews.xiaojinpro.top',
    'api-domain': 'goodnews-api.xiaojins.com',
    'backend-port': '4017',
  });
  const files = generate(c, false);
  const project = JSON.parse(fs.readFileSync(path.join(root, 'deploy/deploy-center/project.json'), 'utf8'));
  const preflight = JSON.parse(
    fs.readFileSync(path.join(root, 'deploy/deployment-closure/preflight.json'), 'utf8'),
  );
  const compose = fs.readFileSync(path.join(root, 'deploy/gcp-vm/compose.yaml'), 'utf8');
  const manifest = fs.readFileSync(path.join(root, 'service.manifest.toml'), 'utf8');
  assert(files.length >= 12, 'expected closure bundle files');
  assert(project.deployment_policy.release_lease_required === true, 'release lease must be required');
  assert(project.deployment_policy.target_side_build_allowed === false, 'target-side build must be disabled');
  assert(preflight.fail_closed_if.includes('db_adoption_plan_missing'), 'db adoption blocker missing');
  assert(preflight.runtime_observation_required.includes('running_image_digest'), 'runtime digest observation missing');
  assert(!/^\s*build\s*:/m.test(compose), 'compose must not include build');
  assert(manifest.includes('[healthcheck]') && manifest.includes('deep = "/api/health/deep"'), 'deep health missing');
  fs.rmSync(root, { recursive: true, force: true });
  return { ok: true, files: files.length };
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args['self-test']) {
    console.log(JSON.stringify(runSelfTest(), null, 2));
    return;
  }
  const config = buildConfig(args);
  const dryRun = Boolean(args['dry-run']);
  const files = generate(config, dryRun);
  console.log(
    JSON.stringify(
      {
        ok: true,
        schema: 'missiond.product-deployment-closure-scaffold-result.v1',
        project: config.projectId,
        root: config.root,
        dry_run: dryRun,
        files,
      },
      null,
      2,
    ),
  );
}

try {
  main();
} catch (error) {
  console.error(
    JSON.stringify(
      {
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      },
      null,
      2,
    ),
  );
  process.exit(1);
}
