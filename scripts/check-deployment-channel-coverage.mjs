#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = process.cwd();

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
  const full = path.join(ROOT, file);
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
