#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = process.cwd();
const XJP_BACKEND_ROOT = '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend';

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const diagnostics = [];
  const compile = spawnSync(process.execPath, ['scripts/compile-v3-runtime.mjs', '--check', '--json'], {
    cwd: ROOT,
    encoding: 'utf8',
  });
  if (compile.status !== 0) {
    diagnostics.push({
      file: 'scripts/compile-v3-runtime.mjs',
      message: `compile-v3-runtime --check failed: ${compile.stderr || compile.stdout}`,
    });
    return finish(diagnostics, opts);
  }
  let result;
  try {
    result = JSON.parse(compile.stdout);
  } catch (err) {
    diagnostics.push({
      file: 'scripts/compile-v3-runtime.mjs',
      message: `compile-v3-runtime --check did not return JSON: ${err.message}`,
    });
    return finish(diagnostics, opts);
  }
  const universeRow = (result.results ?? []).find((row) => row.id === 'universe');
  const universePath = universeRow?.path;
  if (!universePath || !fs.existsSync(universePath)) {
    diagnostics.push({
      file: 'compiled-project-universe.json',
      message: 'compiled universe path missing from compile-v3-runtime result',
    });
    return finish(diagnostics, opts);
  }
  const universe = JSON.parse(fs.readFileSync(universePath, 'utf8'));
  checkUniverseProjection(diagnostics, universe);
  checkWiring(diagnostics);
  checkCodebaseDeployClosureBoundaries(diagnostics);
  finish(diagnostics, opts);
}

function checkUniverseProjection(diagnostics, universe) {
  const payload = universe.payload ?? {};
  const services = Array.isArray(payload.services) ? payload.services : [];
  const channels = Array.isArray(payload.deployment_channels) ? payload.deployment_channels : [];
  const serviceById = new Map(services.map((service) => [service.id, service]));
  if (channels.length === 0) {
    diagnostics.push({
      file: 'compiled-project-universe.json',
      message: 'payload.deployment_channels must be populated',
    });
  }
  if ((payload.deployment_channel_diagnostics ?? []).length > 0) {
    diagnostics.push({
      file: 'compiled-project-universe.json',
      message: `deployment_channel_diagnostics must be empty: ${JSON.stringify(payload.deployment_channel_diagnostics)}`,
    });
  }
  for (const service of services) {
    if (!serviceNeedsBuild(service)) continue;
    const buildChannels = (service.deployment_channels ?? []).filter((channel) => channel.surface === 'build');
    if (buildChannels.length !== 1) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: service.id,
        message: `${service.id} must have exactly one build channel; got ${buildChannels.length}`,
      });
    }
  }
  for (const channel of channels) {
    if (!stringValue(channel.id)) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: channel.service_id,
        message: 'deployment channel must expose id',
      });
    }
    if (!stringValue(channel.service_id)) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: channel.service_id,
        message: `${channel.id ?? '<unknown>'} deployment channel must expose service_id`,
      });
    }
    if (!stringValue(channel.project_id)) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: channel.service_id,
        message: `${channel.id ?? channel.service_id ?? '<unknown>'} deployment channel must expose project_id`,
      });
    }
    if (!stringValue(channel.surface)) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: channel.service_id,
        message: `${channel.id ?? channel.service_id ?? '<unknown>'} deployment channel must expose surface`,
      });
    }
    if (channel.channel_kind === 'github_actions') {
      if (!channel.workflow) {
        diagnostics.push({
          file: 'compiled-project-universe.json',
          service: channel.service_id,
          message: `${channel.service_id} github_actions build channel must name workflow`,
        });
      }
      if (!channel.source_ref || !fs.existsSync(channel.source_ref)) {
        diagnostics.push({
          file: 'compiled-project-universe.json',
          service: channel.service_id,
          message: `${channel.service_id} github_actions source_ref must exist: ${channel.source_ref ?? '<missing>'}`,
        });
      }
    }
    if (channel.channel_kind === 'native_workflow' && channel.target_side_build_prohibited !== true) {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: channel.service_id,
        message: `${channel.service_id} native_workflow channel must prohibit target-side build`,
      });
    }
    if (channel.surface === 'build' && channel.channel_kind === 'native_workflow') {
      for (const field of ['builder', 'executor', 'source_sync', 'artifact_lane', 'dockerfile', 'image']) {
        if (!channel[field]) {
          diagnostics.push({
            file: 'compiled-project-universe.json',
            service: channel.service_id,
            message: `${channel.service_id} native_workflow channel must expose ${field}`,
          });
        }
      }
      checkNativeWorkflowProjectConfig(diagnostics, serviceById.get(channel.service_id), channel);
    }
    if (channel.surface === 'build' && channel.channel_kind === 'github_actions') {
      diagnostics.push({
        file: 'compiled-project-universe.json',
        service: channel.service_id,
        message: `${channel.service_id} backend build channel must use codebase self-managed native_workflow, not github_actions`,
      });
    }
  }
  requireChannel(diagnostics, channels, 'good-things-daily', 'build', 'native_workflow');
  requireChannel(diagnostics, channels, 'good-things-daily', 'runtime', 'gcp_vm');
  requireChannel(diagnostics, channels, 'good-things-daily', 'frontend', 'vercel');
  requireChannel(diagnostics, channels, 'asr', 'build', 'native_workflow');
  requireChannel(diagnostics, channels, 'asr', 'runtime', 'deploy_center_runtime');
  requireChannel(diagnostics, channels, 'asr', 'frontend', 'vercel');
  for (const serviceId of [
    'auth',
    'deploy-center',
    'search-center',
    'payments',
    'xjp-project-universe',
    'xjp-image-service',
    'xjp-video-service',
    'xjp-domain-service',
    'xjp-mail-service',
    'wepub',
    'secret-store',
    'secret-store-cn',
  ]) {
    requireChannel(diagnostics, channels, serviceId, 'build', 'native_workflow');
  }
}

function checkNativeWorkflowProjectConfig(diagnostics, service, channel) {
  const root = stringValue(service?.root);
  const serviceId = stringValue(channel.service_id);
  if (!root || !serviceId) return;
  const configPath = resolveDeployCenterProjectConfig(root, serviceId);
  if (!configPath) {
    diagnostics.push({
      file: 'deploy/deploy-center/project.json',
      service: serviceId,
      message: `${serviceId} native_workflow channel must have deploy/deploy-center/project.json or project.${serviceId}.json`,
    });
    return;
  }
  let config;
  try {
    config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
  } catch (err) {
    diagnostics.push({
      file: configPath,
      service: serviceId,
      message: `${serviceId} deploy-center project config is not valid JSON: ${err.message}`,
    });
    return;
  }
  const build = config?.stages?.build;
  const deployType = stringValue(build?.config?.deploy_type);
  if (build?.enabled === false || deployType !== 'native_workflow') {
    diagnostics.push({
      file: configPath,
      service: serviceId,
      message: `${serviceId} deploy-center build stage must be enabled with config.deploy_type=native_workflow`,
    });
  }
  const expectedExecutorProject = stringValue(config?.project?.slug);
  const actualExecutorProject = stringValue(build?.executor_project);
  if (expectedExecutorProject && actualExecutorProject !== expectedExecutorProject) {
    diagnostics.push({
      file: configPath,
      service: serviceId,
      message: `${serviceId} native build must keep executor_project=${expectedExecutorProject}; got ${actualExecutorProject || '<missing>'}`,
    });
  }
  if (isXiaojinproBackendRoot(root)) {
    const buildArgs = build?.config?.build_args ?? {};
    for (const [key, expected] of [
      ['REGISTRY_PREFIX', '192.168.1.20:8880/dockerhub-cache/'],
      ['CARGO_REGISTRY', 'skip'],
      ['RUSTC_WRAPPER_BIN', ''],
    ]) {
      if (buildArgs[key] !== expected) {
        diagnostics.push({
          file: configPath,
          service: serviceId,
          message: `${serviceId} deploy-center native build config must set build_args.${key}=${JSON.stringify(expected)}`,
        });
      }
    }
  }
  for (const field of ['image', 'dockerfile']) {
    if (!stringValue(build?.config?.[field])) {
      diagnostics.push({
        file: configPath,
        service: serviceId,
        message: `${serviceId} deploy-center native build config must include ${field}`,
      });
    }
  }
  const native = build?.config?.native_workflow ?? {};
  if (!stringValue(native.trigger_adapter_status)) {
    diagnostics.push({
      file: configPath,
      service: serviceId,
      message: `${serviceId} deploy-center native build config must mark trigger_adapter_status`,
    });
  }
  checkNativeWorkflowRunnerPin(diagnostics, configPath, serviceId, build?.config ?? {}, native);
  checkCodebaseAuthorityConfig(diagnostics, configPath, serviceId, build?.config ?? {}, native);
  checkFrontendBuildRunnerPin(diagnostics, configPath, serviceId, config);
}

function checkFrontendBuildRunnerPin(diagnostics, configPath, serviceId, config) {
  const frontend = config?.stages?.frontend;
  if (!frontend || frontend.enabled === false) return;
  const frontendConfig = frontend.config ?? {};
  if (stringValue(frontendConfig.deploy_type) !== 'cn_frontend') return;
  checkNativeWorkflowRunnerPin(
    diagnostics,
    configPath,
    `${serviceId}:frontend`,
    frontendConfig,
    frontendConfig.native_workflow ?? {},
  );
}

function checkNativeWorkflowRunnerPin(diagnostics, configPath, serviceId, buildConfig, native) {
  const runnerPin = stringValue(native.runner_agent_id)
    ?? stringValue(native.build_runner_agent_id)
    ?? stringValue(buildConfig.runner_agent_id)
    ?? stringValue(buildConfig.build_runner_agent_id);
  if (!runnerPin) return;
  const rationale = stringValue(native.runner_pin_rationale)
    ?? stringValue(buildConfig.runner_pin_rationale)
    ?? stringValue(native.runner_pin_scope)
    ?? stringValue(buildConfig.runner_pin_scope);
  if (rationale) return;
  diagnostics.push({
    file: configPath,
    service: serviceId,
    message: `${serviceId} generic native Linux build must not pin runner_agent_id=${runnerPin}; use runner_labels/required_capabilities for the 10900KF+12900KF pool or add runner_pin_rationale`,
  });
}

function checkCodebaseAuthorityConfig(diagnostics, configPath, serviceId, buildConfig, native) {
  if (stringValue(buildConfig.source_provider) !== 'xjp-codebase') return;
  const canonical = stringValue(buildConfig.canonical_remote);
  const repoUrl = stringValue(buildConfig.repo_url);
  const mirror = stringValue(buildConfig.mirror_remote);
  if (!canonical || !canonical.startsWith('ssh://git@codebase.xiaojins.com:2222/')) {
    diagnostics.push({
      file: configPath,
      service: serviceId,
      message: `${serviceId} xjp-codebase build must declare canonical_remote under codebase.xiaojins.com:2222`,
    });
  }
  if (!repoUrl || repoUrl !== canonical) {
    diagnostics.push({
      file: configPath,
      service: serviceId,
      message: `${serviceId} xjp-codebase build repo_url must equal canonical_remote`,
    });
  }
  if (!mirror || !mirror.includes('github.com/')) {
    diagnostics.push({
      file: configPath,
      service: serviceId,
      message: `${serviceId} xjp-codebase build must keep GitHub only as mirror_remote evidence`,
    });
  }
  const requiredEnv = new Set(arrayStrings(native.runner_required_env));
  for (const envName of ['CODEBASE_SSH_PRIVATE_KEY', 'CODEBASE_KNOWN_HOSTS']) {
    if (!requiredEnv.has(envName)) {
      diagnostics.push({
        file: configPath,
        service: serviceId,
        message: `${serviceId} xjp-codebase build must require runner env ${envName}`,
      });
    }
  }
  const secretEnv = native.secret_env && typeof native.secret_env === 'object' ? native.secret_env : {};
  for (const envName of ['CODEBASE_SSH_PRIVATE_KEY', 'CODEBASE_KNOWN_HOSTS']) {
    if (!stringValue(secretEnv[envName])) {
      diagnostics.push({
        file: configPath,
        service: serviceId,
        message: `${serviceId} xjp-codebase build must map secret_env.${envName}`,
      });
    }
  }
}

function resolveDeployCenterProjectConfig(root, serviceId) {
  const candidates = [
    path.join(root, `deploy/deploy-center/project.${serviceId}.json`),
    path.join(root, 'deploy/deploy-center/project.json'),
  ];
  return candidates.find((candidate) => fs.existsSync(candidate)) ?? null;
}

function isXiaojinproBackendRoot(root) {
  return path.resolve(root).startsWith('/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend');
}

function checkWiring(diagnostics) {
  requireText(diagnostics, '.missiond/v3/shards/universe/service-runtime.lisp', [
    '(deployment-channel-plane',
    ':schema "missiond.deployment-channel-plane.v1"',
    ':merge-precedence [explicit-v3-deployment-channels project-local-deploy-center-config repo-workflow-inference live-observed-annotation]',
    'runner_agent_id is a singleton affinity override and requires runner_pin_rationale evidence',
  ]);
  requireText(diagnostics, 'crates/missiond-daemon/src/handlers/knowledge/project.rs', [
    'deployment_channels::handle_deployment_channels',
    'deployment_channels::handle_reconcile_deployment_channels',
  ]);
  requireText(diagnostics, 'crates/missiond-daemon/src/handlers/knowledge/project/deployment_channels.rs', [
    'MISSIOND_DEPLOY_CENTER_BASE_URL',
    'MISSIOND_DEPLOY_CENTER_READ_TOKEN',
    'read token not configured',
  ]);
  requireText(diagnostics, 'packages/board/src/app/api/projects/route.ts', [
    "action: 'deployment_channels'",
    "kind: 'compiled'",
    'deploymentChannels: explicitChannels.length',
  ]);
  requireText(diagnostics, 'packages/board/src/components/SystemDashboard.tsx', [
    'DeploymentChannelSummary',
    'GitHub Actions',
    'Native Runner',
    'obs ',
    'drift ',
  ]);
}

function checkCodebaseDeployClosureBoundaries(diagnostics) {
  const deployCenterApiMod = path.join(XJP_BACKEND_ROOT, 'services/deploy-center/src/api/mod.rs');
  requireText(diagnostics, deployCenterApiMod, [
    '.route("/closure/:project", get(get_project_closure))',
    '.route("/evidence/:release_id", get(get_release_evidence))',
    '.route("/releases/:id/evidence", post(append_release_evidence))',
    '.route("/releases/:id/close", post(close_release))',
    '"/codebase/overview"',
    'native_control_plane_overview',
  ]);
  requireText(diagnostics, path.join(XJP_BACKEND_ROOT, 'services/deploy-center/src/api/release_closure.rs'), [
    'deploy-center.project-closure.v1',
    'ReleaseEvidence+ClosureVerdict',
    'deploy-center.release-evidence.v1',
    'deploy-center.closure-verdict.v1',
  ]);
  requireText(diagnostics, path.join(XJP_BACKEND_ROOT, 'services/deploy-center/src/api/codebase_runner.rs'), [
    'deploy-center.codebase-compatibility-boundary.v1',
    'legacy-read-through',
    'https://code.xiaojins.com/api/codebase',
    'migration_expiry',
    'source-authority',
    'push-success-online-semantics',
  ]);
  requireText(diagnostics, path.join(XJP_BACKEND_ROOT, 'services/code-center/src/lib.rs'), [
    '.nest("/api/code", api::routes())',
    '.nest("/api/codebase", codebase::api::routes())',
  ]);
  requireText(diagnostics, path.join(XJP_BACKEND_ROOT, 'services/code-center/src/codebase/api.rs'), [
    'xjp-codebase.overview.v1',
    'source_authority',
    'legacy_code_api_prefix',
    'git_ssh_endpoint_template',
    'ReleasePlan+ReleaseEvidence+ClosureVerdict',
  ]);
  requireText(diagnostics, path.join(XJP_BACKEND_ROOT, 'services/code-center/src/codebase/deploy.rs'), [
    'trigger_source: "codebase-push"',
    '"source_authority": "xjp-codebase"',
    '"canonical_remote": repository.canonical_remote',
    '"changed_paths": event.changed_paths',
    '"deploy_project_slug": binding.deploy_project_slug',
  ]);
  requireText(diagnostics, path.join(XJP_BACKEND_ROOT, 'services/code-center/service.manifest.toml'), [
    'path = "/api/code/health"',
    'path = "/api/codebase/health"',
    '[env.optional.CODEBASE_SERVICE_TOKEN_KEY]',
    '[env.optional.SECRET_STORE_API_KEY]',
  ]);
  requireText(diagnostics, '.missiond/v3/shards/deployment-closure-plane.lisp', [
    'POST /api/deploy/releases/:release_id/evidence',
    'POST /api/deploy/releases/:release_id/close',
    'Deploy Center /api/deploy/codebase/* is legacy-read-through',
  ]);
  requireText(diagnostics, '.missiond/v3/shards/universe/service-runtime.lisp', [
    ':codebase-boundary (:schema "missiond.codebase-boundary.v1"',
    ':canonical-api-prefix "/api/codebase"',
    ':deploy-center-legacy-prefix "/api/deploy/codebase"',
    ':legacy-migration-expiry "2026-06-30"',
  ]);
}

function serviceNeedsBuild(service) {
  const environment = stringValue(service.environment);
  if (environment === 'local-dev') return false;
  const deployment = service.deployment ?? {};
  const substrate = stringValue(deployment.substrate);
  if (!substrate) return Boolean(service.backend);
  if (['vercel', 'lovable-or-static-host', 'local-node', 'gcp-caddy-edge'].includes(substrate)) {
    return false;
  }
  return Boolean(service.backend) || ['deploy-center', 'gcp-vm', 'aliyun-ecs', 'kubernetes'].includes(substrate);
}

function requireChannel(diagnostics, channels, serviceId, surface, kind) {
  const found = channels.some((channel) => (
    channel.service_id === serviceId
    && channel.surface === surface
    && channel.channel_kind === kind
  ));
  if (!found) {
    diagnostics.push({
      file: 'compiled-project-universe.json',
      service: serviceId,
      message: `${serviceId} must have ${surface}:${kind} deployment channel`,
    });
  }
}

function requireText(diagnostics, file, needles) {
  const full = path.isAbsolute(file) ? file : path.join(ROOT, file);
  const text = fs.existsSync(full) ? fs.readFileSync(full, 'utf8') : '';
  for (const needle of needles) {
    if (!text.includes(needle)) {
      diagnostics.push({ file, message: `missing required text: ${needle}` });
    }
  }
}

function stringValue(value) {
  return typeof value === 'string' && value.trim() ? value : null;
}

function arrayStrings(value) {
  if (!Array.isArray(value)) return [];
  return value.filter((item) => typeof item === 'string' && item.trim()).map((item) => item.trim());
}

function parseArgs(argv) {
  return { json: argv.includes('--json') };
}

function finish(diagnostics, opts) {
  const ok = diagnostics.length === 0;
  if (opts.json) {
    console.log(JSON.stringify({
      ok,
      diagnostics,
    }, null, 2));
  } else if (ok) {
    console.log('deployment channel coverage ok');
  } else {
    for (const diag of diagnostics) {
      console.error(`${diag.file}: ${diag.message}`);
    }
  }
  process.exit(ok ? 0 : 1);
}

main();
